use async_trait::async_trait;
use devenv_activity::{Activity, ProcessStatus};
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use nix::sys::signal::{self, Signal as NixSignal};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// Commands that can be sent to control processes
#[derive(Debug, Clone)]
pub enum ProcessCommand {
    /// Restart a process by name
    Restart(String),
}
use watchexec::Watchexec;
use watchexec_filterer_globset::GlobsetFilterer;
use watchexec_supervisor::{
    ProcessEnd, Signal,
    command::{Command, Program, Shell, SpawnOptions},
    job::{CommandState, Job, start_job},
};

use crate::config::{ProcessConfig, RestartPolicy};
use crate::notify_socket::{NotifyMessage, NotifySocket};
use crate::pid::{self, PidStatus};
use crate::socket_activation::{ProcessSetupWrapper, activation_from_listen};
use crate::{ProcessManager, StartOptions};

/// State file for persisting process information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessManagerState {
    pub state_dir: PathBuf,
    pub processes: HashMap<String, ProcessState>,
}

/// State information for a single process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessState {
    pub name: String,
    pub pid: u32,
}

/// Supervision status for a process
enum SupervisorStatus {
    /// Waiting for process to become ready (initial or after restart).
    WaitingForReady { watchdog_deadline: Option<Instant> },
    /// Process is ready. Watchdog deadline active if configured.
    Ready { watchdog_deadline: Option<Instant> },
}

/// Action to take after a process exits or watchdog timeout
enum ExitAction {
    Restart,
    GiveUp,
    Stop,
}

/// Handle to a managed process with supervision state
pub struct Process {
    /// The watchexec job for process control
    pub job: Arc<Job>,
    /// Activity for tracking process lifecycle
    pub activity: Activity,
    /// Output reader tasks (stdout, stderr)
    pub output_readers: Option<(JoinHandle<()>, JoinHandle<()>)>,
    /// Notify socket for systemd-style notifications (owned here to keep alive)
    pub notify_socket: Option<Arc<NotifySocket>>,
    /// Ready state for signaling when process becomes ready (READY=1 or TCP probe)
    pub ready_state: tokio::sync::watch::Sender<bool>,

    // Sub-task handles (for cleanup)
    tcp_probe_task: Option<JoinHandle<()>>,
    file_watcher_task: Option<JoinHandle<()>>,
    notify_forwarder_task: Option<JoinHandle<()>>,

    // Supervision state
    config: ProcessConfig,
    status: SupervisorStatus,
    restart_count: usize,
    ready_signaled: bool,
    /// Initiated restart in progress — absorb next exit
    restarting: bool,
    /// Process exited and won't restart, kept for TUI visibility
    stopped: bool,
    watchdog_timeout: Option<Duration>,
    watchdog_require_ready: bool,
    /// Cooldown: ignore file watch events until this instant
    file_watch_cooldown: Option<Instant>,
}

impl Process {
    /// Determine the action to take based on restart policy and exit status.
    fn check_exit_policy(&mut self, is_failure: bool) -> ExitAction {
        let should_restart = match self.config.restart {
            RestartPolicy::Never => false,
            RestartPolicy::Always => true,
            RestartPolicy::OnFailure => is_failure,
        };

        if !should_restart {
            return ExitAction::Stop;
        }

        if let Some(max) = self.config.max_restarts
            && self.restart_count >= max
        {
            return ExitAction::GiveUp;
        }

        self.restart_count += 1;
        ExitAction::Restart
    }

    /// Handle a notify message from the process.
    fn handle_notify_message(&mut self, msg: &NotifyMessage) {
        let name = &self.config.name;
        match msg {
            NotifyMessage::Ready => {
                info!("Process {} signaled ready", name);
                self.activity.log("Process signaled ready");
                self.activity.set_status(ProcessStatus::Ready);
                self.restart_count = 0;
                if !self.ready_signaled {
                    self.ready_signaled = true;
                    let _ = self.ready_state.send(true);
                }
                self.status = SupervisorStatus::Ready {
                    watchdog_deadline: self.new_watchdog_deadline(),
                };
            }
            NotifyMessage::Watchdog => {
                debug!("Watchdog ping from {}", name);
                self.restart_count = 0;
                if let SupervisorStatus::Ready {
                    ref mut watchdog_deadline,
                } = self.status
                {
                    if let Some(timeout) = self.watchdog_timeout {
                        *watchdog_deadline = Some(Instant::now() + timeout);
                    }
                } else {
                    debug!("Ignoring watchdog ping from {} (not in Ready state)", name);
                }
            }
            NotifyMessage::Status(status) => {
                debug!("Status from {}: {}", name, status);
                self.activity.log(format!("Status: {}", status));
            }
            NotifyMessage::Stopping => {
                debug!("Process {} signaled stopping", name);
                self.activity.log("Process signaled stopping");
            }
            NotifyMessage::Reloading => {
                debug!("Process {} signaled reloading", name);
                self.activity.log("Process reloading configuration");
            }
            NotifyMessage::Unknown(s) => {
                debug!("Unknown notify message from {}: {}", name, s);
            }
        }
    }

    /// Handle watchdog timeout — check max_restarts and return action.
    fn handle_watchdog_timeout(&mut self) -> ExitAction {
        let name = &self.config.name;
        warn!("Watchdog timeout for process {}", name);
        self.activity
            .error("Watchdog timeout - no heartbeat received");

        if let Some(max) = self.config.max_restarts
            && self.restart_count >= max
        {
            warn!(
                "Process {} reached max restarts ({}) after watchdog timeout, giving up",
                name, max
            );
            self.activity
                .error(format!("Max restarts ({}) reached, giving up", max));
            return ExitAction::GiveUp;
        }

        self.restart_count += 1;
        info!(
            "Restarting process {} due to watchdog timeout (attempt {})",
            name, self.restart_count
        );
        self.activity.log(format!(
            "Restarting due to watchdog timeout (attempt {})",
            self.restart_count
        ));
        ExitAction::Restart
    }

    /// Reset supervision state for a user-initiated restart (Ctrl+R).
    fn reset_for_restart(&mut self) {
        self.restart_count = 0;
        self.ready_signaled = false;
        let _ = self.ready_state.send(false);
        self.restarting = false;
        self.stopped = false;
        self.file_watch_cooldown = None;
        self.status = self.initial_status();
    }

    /// Compute the initial supervision status based on config.
    fn initial_status(&self) -> SupervisorStatus {
        if !self.watchdog_require_ready {
            SupervisorStatus::Ready {
                watchdog_deadline: self.new_watchdog_deadline(),
            }
        } else {
            SupervisorStatus::WaitingForReady {
                watchdog_deadline: None,
            }
        }
    }

    /// Compute a new watchdog deadline from now, if configured.
    fn new_watchdog_deadline(&self) -> Option<Instant> {
        self.watchdog_timeout.map(|t| Instant::now() + t)
    }

    /// Return the current watchdog deadline, if any.
    fn watchdog_deadline(&self) -> Option<Instant> {
        match &self.status {
            SupervisorStatus::Ready { watchdog_deadline }
            | SupervisorStatus::WaitingForReady { watchdog_deadline } => *watchdog_deadline,
        }
    }

    /// Abort process-lifecycle-dependent sub-tasks (TCP probe, notify forwarder).
    /// The file watcher is left running since it watches the filesystem, not the process.
    fn abort_process_subtasks(&mut self) {
        if let Some(task) = self.tcp_probe_task.take() {
            task.abort();
        }
        if let Some(task) = self.notify_forwarder_task.take() {
            task.abort();
        }
    }

    /// Abort all sub-tasks (TCP probe, file watcher, notify forwarder).
    fn abort_subtasks(&mut self) {
        self.abort_process_subtasks();
        if let Some(task) = self.file_watcher_task.take() {
            task.abort();
        }
    }
}

/// Native process manager using watchexec-supervisor
pub struct NativeProcessManager {
    jobs: Arc<RwLock<HashMap<String, Process>>>,
    state_dir: PathBuf,
    /// Process configurations (populated when processes are started)
    process_configs: RwLock<HashMap<String, ProcessConfig>>,
    /// Command receiver for process control (restart, etc.)
    command_rx: Arc<tokio::sync::Mutex<Option<mpsc::Receiver<ProcessCommand>>>>,
    /// Parent activity for grouping all processes under "Starting processes"
    processes_activity: Arc<RwLock<Option<Activity>>>,
    /// Channel for notify socket messages from forwarder tasks
    notify_tx: mpsc::Sender<(String, Vec<NotifyMessage>)>,
    #[allow(clippy::type_complexity)]
    notify_rx: tokio::sync::Mutex<Option<mpsc::Receiver<(String, Vec<NotifyMessage>)>>>,
    /// Channel for file watch events from watcher tasks
    file_watch_tx: mpsc::Sender<String>,
    file_watch_rx: tokio::sync::Mutex<Option<mpsc::Receiver<String>>>,
}

impl NativeProcessManager {
    /// Create a new native process manager
    pub fn new(
        state_dir: PathBuf,
        process_configs: HashMap<String, ProcessConfig>,
    ) -> Result<Self> {
        std::fs::create_dir_all(&state_dir).into_diagnostic()?;

        let (notify_tx, notify_rx) = mpsc::channel(64);
        let (file_watch_tx, file_watch_rx) = mpsc::channel(64);

        Ok(Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            state_dir,
            process_configs: RwLock::new(process_configs),
            command_rx: Arc::new(tokio::sync::Mutex::new(None)),
            processes_activity: Arc::new(RwLock::new(None)),
            notify_tx,
            notify_rx: tokio::sync::Mutex::new(Some(notify_rx)),
            file_watch_tx,
            file_watch_rx: tokio::sync::Mutex::new(Some(file_watch_rx)),
        })
    }

    /// Set the command receiver for process control
    pub async fn set_command_receiver(&self, rx: mpsc::Receiver<ProcessCommand>) {
        let mut guard = self.command_rx.lock().await;
        *guard = Some(rx);
    }

    /// Get the state directory
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// Path to the manager PID file
    pub fn manager_pid_file(&self) -> PathBuf {
        self.state_dir.join("native-manager.pid")
    }

    /// Start a command with the given configuration
    ///
    /// Returns a reference to the job's Arc for status checking.
    pub async fn start_command(
        &self,
        config: &ProcessConfig,
        parent_id: Option<u64>,
    ) -> Result<Arc<Job>> {
        debug!("Starting command '{}': {}", config.name, config.exec);

        if config.pseudo_terminal {
            bail!(
                "Process '{}' requested pseudo_terminal, but the native process manager does not support PTY yet",
                config.name
            );
        }

        // Store config for restart support
        {
            let mut configs = self.process_configs.write().await;
            configs.insert(config.name.clone(), config.clone());
        }

        // Extract ports from listen config and allocated ports
        let mut ports: Vec<String> = config
            .listen
            .iter()
            .filter_map(|spec| {
                // Extract port from address like "127.0.0.1:8080" -> "name:8080"
                spec.address.as_ref().and_then(|addr| {
                    addr.rsplit(':')
                        .next()
                        .map(|port| format!("{}:{}", spec.name, port))
                })
            })
            .collect();
        // Add allocated ports not already covered by listen specs
        let listen_names: std::collections::HashSet<&str> =
            config.listen.iter().map(|s| s.name.as_str()).collect();
        for (name, port) in &config.ports {
            if !listen_names.contains(name.as_str()) {
                ports.push(format!("{}:{}", name, port));
            }
        }

        // Create activity for tracking this process
        let mut builder = Activity::process(&config.name)
            .command(&config.exec)
            .ports(ports);
        if let Some(pid) = parent_id {
            builder = builder.parent(Some(pid));
        }
        let activity = builder.start();

        // Create notify socket if configured
        let notify_socket = if config.notify.as_ref().is_some_and(|n| n.enable) {
            let socket = NotifySocket::new(&self.state_dir, &config.name).await?;
            info!(
                "Created notify socket for {} at {}",
                config.name,
                socket.path().display()
            );
            Some(Arc::new(socket))
        } else {
            None
        };

        // Get watchdog interval if configured
        let watchdog_usec = config.watchdog.as_ref().map(|w| w.usec);

        // Build the command (creates log directory and wrapper script)
        let (cmd, stdout_log, stderr_log) = self.build_command(
            config,
            notify_socket.as_ref().map(|s| s.path()),
            watchdog_usec,
        )?;

        // Truncate log files if they exist
        let _ = std::fs::write(&stdout_log, "");
        let _ = std::fs::write(&stderr_log, "");

        let (job, _task) = start_job(cmd);
        let job = Arc::new(job);

        // Setup socket activation and/or capabilities if configured
        let has_sockets = !config.listen.is_empty();
        let has_caps = !config.linux.capabilities.add.is_empty();

        if has_sockets || has_caps {
            let fds = if has_sockets {
                info!("Setting up socket activation for {}", config.name);
                let spec = activation_from_listen(&config.listen)?;
                let activated = spec.create_fds()?;
                debug!(
                    "Created {} activation sockets for {}",
                    activated.fds().len(),
                    config.name
                );
                activated.into_fds()
            } else {
                Vec::new()
            };

            if has_caps {
                info!(
                    "Setting up capabilities for {}: {:?}",
                    config.name, config.linux.capabilities.add
                );
            }

            let capabilities = config.linux.capabilities.clone();
            job.set_spawn_hook(move |command_wrap, _ctx| {
                command_wrap.wrap(ProcessSetupWrapper::new(fds.clone(), capabilities.clone()));
            });
        }

        job.start().await;

        // Spawn file tailers to emit output to activity
        let stdout_tailer = Self::spawn_file_tailer(stdout_log, activity.clone(), false);
        let stderr_tailer = Self::spawn_file_tailer(stderr_log, activity.clone(), true);

        // Create ready state for signaling when process becomes ready
        let (ready_state, _ready_rx) = tokio::sync::watch::channel(false);

        // Extract watchdog config
        let watchdog_timeout = config
            .watchdog
            .as_ref()
            .map(|w| Duration::from_micros(w.usec));
        let watchdog_require_ready = config
            .watchdog
            .as_ref()
            .map(|w| w.require_ready)
            .unwrap_or(true);

        // Spawn TCP probe task if needed
        let tcp_probe_task = if !config.listen.is_empty()
            && config.notify.as_ref().is_none_or(|n| !n.enable)
        {
            config
                .listen
                .iter()
                .find_map(|spec| {
                    if spec.kind == crate::config::ListenKind::Tcp {
                        spec.address.clone()
                    } else {
                        None
                    }
                })
                .map(|address| {
                    let ready_state = ready_state.clone();
                    let probe_name = config.name.clone();
                    let probe_activity = activity.clone();
                    tokio::spawn(async move {
                        debug!("Starting TCP probe for {} at {}", probe_name, address);
                        loop {
                            match tokio::net::TcpStream::connect(&address).await {
                                Ok(_) => {
                                    info!("TCP probe succeeded for {} at {}", probe_name, address);
                                    probe_activity.log("TCP probe succeeded - process ready");
                                    probe_activity.set_status(ProcessStatus::Ready);
                                    let _ = ready_state.send(true);
                                    break;
                                }
                                Err(_) => {
                                    tokio::time::sleep(Duration::from_millis(100)).await;
                                }
                            }
                        }
                    })
                })
        } else {
            None
        };

        // Spawn file watcher task if watch paths are configured
        let file_watcher_task = if !config.watch.paths.is_empty() {
            let paths = config.watch.paths.clone();
            let extensions = config.watch.extensions.clone();
            let ignore = config.watch.ignore.clone();
            let watch_name = config.name.clone();
            let tx = self.file_watch_tx.clone();

            Some(tokio::spawn(async move {
                let ignores: Vec<(String, Option<PathBuf>)> = ignore
                    .iter()
                    .map(|pattern| {
                        let glob_pattern = if pattern.contains('/') || pattern.starts_with("**") {
                            pattern.clone()
                        } else {
                            format!("**/{}", pattern)
                        };
                        (glob_pattern, None)
                    })
                    .collect();

                let origin = paths.first().cloned().unwrap_or_else(|| PathBuf::from("."));

                let filterer = match GlobsetFilterer::new(
                    &origin,
                    std::iter::empty::<(String, Option<PathBuf>)>(),
                    ignores,
                    std::iter::empty::<PathBuf>(),
                    std::iter::empty(),
                    extensions.iter().map(std::ffi::OsString::from),
                )
                .await
                {
                    Ok(f) => Arc::new(f),
                    Err(e) => {
                        warn!("Failed to create filterer for {}: {}", watch_name, e);
                        return;
                    }
                };

                let name_for_action = watch_name.clone();
                let wx = match Watchexec::new(move |action| {
                    if action.events.iter().any(|e| e.paths().next().is_some()) {
                        let _ = tx.try_send(name_for_action.clone());
                    }
                    action
                }) {
                    Ok(wx) => wx,
                    Err(e) => {
                        warn!("Failed to create file watcher for {}: {}", watch_name, e);
                        return;
                    }
                };

                wx.config.pathset(paths.iter().map(|p| p.as_path()));
                wx.config.filterer(filterer);
                wx.config.throttle(Duration::from_millis(500));

                let mut watch_info = format!(
                    "File watcher started for {} watching {:?}",
                    watch_name, paths
                );
                if !extensions.is_empty() {
                    watch_info.push_str(&format!(" (extensions: {:?})", extensions));
                }
                if !ignore.is_empty() {
                    watch_info.push_str(&format!(" (ignoring {:?})", ignore));
                }
                info!("{}", watch_info);

                if let Err(e) = wx.main().await {
                    warn!("File watcher for {} stopped: {}", watch_name, e);
                }
            }))
        } else {
            None
        };

        // Spawn notify forwarder task
        let notify_forwarder_task = if let Some(socket) = notify_socket.clone() {
            let name = config.name.clone();
            let tx = self.notify_tx.clone();
            Some(tokio::spawn(async move {
                loop {
                    match socket.recv().await {
                        Ok(messages) => {
                            if tx.send((name.clone(), messages)).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            debug!("Notify socket error for {}: {}", name, e);
                            break;
                        }
                    }
                }
            }))
        } else {
            None
        };

        // Build initial supervision status
        let initial_status = if !watchdog_require_ready {
            SupervisorStatus::Ready {
                watchdog_deadline: watchdog_timeout.map(|t| Instant::now() + t),
            }
        } else {
            SupervisorStatus::WaitingForReady {
                watchdog_deadline: None,
            }
        };

        // Store the process
        let job_clone = job.clone();
        let mut jobs = self.jobs.write().await;
        jobs.insert(
            config.name.clone(),
            Process {
                job,
                activity,
                output_readers: Some((stdout_tailer, stderr_tailer)),
                notify_socket,
                ready_state,
                tcp_probe_task,
                file_watcher_task,
                notify_forwarder_task,
                config: config.clone(),
                status: initial_status,
                restart_count: 0,
                ready_signaled: false,
                restarting: false,
                stopped: false,
                watchdog_timeout,
                watchdog_require_ready,
                file_watch_cooldown: None,
            },
        );

        info!("Command '{}' started", config.name);
        Ok(job_clone)
    }

    /// Spawn a task that tails a log file and emits lines to activity
    fn spawn_file_tailer(path: PathBuf, activity: Activity, is_stderr: bool) -> JoinHandle<()> {
        use tokio::io::AsyncSeekExt;

        tokio::spawn(async move {
            // File is already created/truncated by start_command before job starts
            let file = match tokio::fs::File::open(&path).await {
                Ok(f) => f,
                Err(e) => {
                    debug!("Failed to open log file {}: {}", path.display(), e);
                    return;
                }
            };

            // Track our position in the file to avoid re-reading on EOF
            let mut position: u64 = 0;
            let mut reader = BufReader::new(file).lines();

            loop {
                match reader.next_line().await {
                    Ok(Some(line)) => {
                        // Update position: line length + newline byte
                        position += line.len() as u64 + 1;
                        if is_stderr {
                            activity.error(&line);
                        } else {
                            activity.log(&line);
                        }
                    }
                    Ok(None) => {
                        // EOF reached, wait a bit and try again (tail -f behavior)
                        tokio::time::sleep(Duration::from_millis(100)).await;

                        // Re-open file and seek to last known position
                        let new_file = match tokio::fs::File::open(&path).await {
                            Ok(f) => f,
                            Err(_) => break,
                        };

                        // Check if file was truncated (e.g., during restart)
                        // If so, reset position to read from the beginning
                        let metadata = match new_file.metadata().await {
                            Ok(m) => m,
                            Err(_) => break,
                        };
                        if metadata.len() < position {
                            // File was truncated, reset to beginning
                            position = 0;
                        }

                        // Seek to where we left off (or beginning if truncated)
                        let mut new_file = new_file;
                        if let Err(e) = new_file.seek(std::io::SeekFrom::Start(position)).await {
                            debug!("Failed to seek in log file {}: {}", path.display(), e);
                            break;
                        }

                        reader = BufReader::new(new_file).lines();
                    }
                    Err(e) => {
                        debug!("Error reading log file {}: {}", path.display(), e);
                        break;
                    }
                }
            }
        })
    }

    /// Build a command from configuration, returning the command and log file paths
    fn build_command(
        &self,
        config: &ProcessConfig,
        notify_socket_path: Option<&Path>,
        watchdog_usec: Option<u64>,
    ) -> Result<(Arc<Command>, PathBuf, PathBuf)> {
        // Create log directory
        let log_dir = self.state_dir.join("logs");
        std::fs::create_dir_all(&log_dir)
            .into_diagnostic()
            .wrap_err("Failed to create logs directory")?;

        // Log file paths
        let stdout_log = log_dir.join(format!("{}.stdout.log", config.name));
        let stderr_log = log_dir.join(format!("{}.stderr.log", config.name));

        // Build shell script that handles environment, cwd, logging, and sudo
        let script = self.build_wrapper_script(
            config,
            &stdout_log,
            &stderr_log,
            notify_socket_path,
            watchdog_usec,
        )?;

        let program = Program::Shell {
            shell: Shell::new("bash"), // Use "bash" from PATH, not "/bin/bash" (NixOS compatible)
            command: script,
            args: vec![],
        };

        let command = Arc::new(Command {
            program,
            options: SpawnOptions {
                session: true,
                ..Default::default()
            },
        });

        Ok((command, stdout_log, stderr_log))
    }

    /// Build a shell wrapper script that handles env vars, cwd, logging, and sudo
    fn build_wrapper_script(
        &self,
        config: &ProcessConfig,
        stdout_log: &Path,
        stderr_log: &Path,
        notify_socket_path: Option<&Path>,
        watchdog_usec: Option<u64>,
    ) -> Result<String> {
        use std::fmt::Write;

        let mut script = String::new();
        writeln!(script, "#!/bin/bash").unwrap();
        writeln!(script, "set -e").unwrap();

        // Set working directory
        if let Some(ref cwd) = config.cwd {
            writeln!(script, "cd {}", shell_escape::escape(cwd.to_string_lossy())).unwrap();
        }

        // Export environment variables
        if !config.env.is_empty() {
            for (key, value) in &config.env {
                writeln!(
                    script,
                    "export {}={}",
                    shell_escape::escape(key.into()),
                    shell_escape::escape(value.into())
                )
                .unwrap();
            }
        }

        // Set notify socket path if configured
        if let Some(path) = notify_socket_path {
            writeln!(
                script,
                "export NOTIFY_SOCKET={}",
                shell_escape::escape(path.to_string_lossy())
            )
            .unwrap();
        }

        // Set watchdog interval if configured
        if let Some(usec) = watchdog_usec {
            writeln!(script, "export WATCHDOG_USEC={}", usec).unwrap();
        }

        // Build the actual command
        let mut cmd = String::new();

        if config.use_sudo {
            // Use sudo with env preservation
            write!(cmd, "sudo -E ").unwrap();
        }

        // Add the command (not escaped - exec can be a shell command with pipes/redirects)
        write!(cmd, "{}", config.exec).unwrap();

        // Add arguments (escaped for safety)
        for arg in &config.args {
            write!(cmd, " {}", shell_escape::escape(arg.into())).unwrap();
        }

        // Redirect output to logs
        writeln!(
            script,
            "{} >> {} 2>> {}",
            cmd,
            shell_escape::escape(stdout_log.to_string_lossy()),
            shell_escape::escape(stderr_log.to_string_lossy())
        )
        .unwrap();

        debug!("Generated wrapper script for {}: {}", config.name, script);
        Ok(script)
    }

    /// Stop a process by name
    pub async fn stop(&self, name: &str) -> Result<()> {
        let mut jobs = self.jobs.write().await;

        if let Some(mut process) = jobs.remove(name) {
            debug!("Stopping process: {}", name);

            // Stopping a process intentionally is not a failure
            process.activity.reset();

            // Abort sub-tasks to prevent restarts and clean up
            process.abort_subtasks();

            // Abort output reader tasks
            if let Some((stdout_reader, stderr_reader)) = process.output_readers.take() {
                stdout_reader.abort();
                stderr_reader.abort();
            }

            // Send terminate signal with grace period
            process
                .job
                .stop_with_signal(Signal::Terminate, Duration::from_secs(5))
                .await;

            info!("Process {} stopped", name);
            Ok(())
        } else {
            bail!("Process {} not found", name)
        }
    }

    /// Stop all processes
    pub async fn stop_all(&self) -> Result<()> {
        let jobs = self.jobs.read().await;
        let job_names: Vec<String> = jobs.keys().cloned().collect();
        drop(jobs); // Release the read lock

        for name in job_names {
            let _ = self.stop(&name).await; // Continue even if one fails
        }
        Ok(())
    }

    /// Get list of running processes
    pub async fn list(&self) -> Vec<String> {
        let jobs = self.jobs.read().await;
        jobs.keys().cloned().collect()
    }

    /// Wait for a process to become ready, avoiding missed early readiness signals.
    pub async fn wait_ready(&self, name: &str) -> Result<()> {
        let ready_state = {
            let jobs = self.jobs.read().await;
            let handle = jobs
                .get(name)
                .ok_or_else(|| miette::miette!("Process {} not found", name))?;
            handle.ready_state.clone()
        };

        let mut ready_rx = ready_state.subscribe();
        if *ready_rx.borrow() {
            return Ok(());
        }

        while ready_rx.changed().await.is_ok() {
            if *ready_rx.borrow() {
                return Ok(());
            }
        }

        bail!("Process {} ready state channel closed", name);
    }

    /// Spawn the event loop as a background tokio task.
    ///
    /// Returns a JoinHandle. The event loop runs until the cancellation token
    /// is cancelled or all processes have stopped. This is useful for tests
    /// that call `start_command()` directly and need supervision running.
    pub async fn spawn_event_loop(
        &self,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> JoinHandle<Result<()>> {
        let jobs = self.jobs.clone();
        let state_dir = self.state_dir.clone();
        let notify_tx = self.notify_tx.clone();
        let notify_rx = self.notify_rx.lock().await.take();
        let file_watch_rx = self.file_watch_rx.lock().await.take();

        let token = cancellation_token.clone();
        tokio::spawn(async move {
            let shutdown = Box::pin(async move { token.cancelled().await });
            Self::event_loop_inner(
                jobs,
                state_dir,
                notify_tx,
                shutdown,
                None,
                notify_rx,
                file_watch_rx,
            )
            .await
        })
    }

    /// Create an exit watcher future for a process.
    /// Returns a future that resolves with the process name when the process exits.
    fn make_exit_watcher(
        name: String,
        job: Arc<Job>,
    ) -> Pin<Box<dyn futures::Future<Output = String> + Send>> {
        Box::pin(async move {
            job.to_wait().await;
            name
        })
    }

    /// Central event loop that handles all supervision, shared by run() and run_foreground().
    async fn event_loop(
        &self,
        shutdown: Pin<Box<dyn futures::Future<Output = ()> + Send>>,
        command_rx: Option<mpsc::Receiver<ProcessCommand>>,
    ) -> Result<()> {
        let notify_rx = self.notify_rx.lock().await.take();
        let file_watch_rx = self.file_watch_rx.lock().await.take();

        Self::event_loop_inner(
            self.jobs.clone(),
            self.state_dir.clone(),
            self.notify_tx.clone(),
            shutdown,
            command_rx,
            notify_rx,
            file_watch_rx,
        )
        .await
    }

    /// Inner event loop implementation that operates on shared state.
    async fn event_loop_inner(
        jobs: Arc<RwLock<HashMap<String, Process>>>,
        state_dir: PathBuf,
        notify_tx: mpsc::Sender<(String, Vec<NotifyMessage>)>,
        mut shutdown: Pin<Box<dyn futures::Future<Output = ()> + Send>>,
        mut command_rx: Option<mpsc::Receiver<ProcessCommand>>,
        mut notify_rx: Option<mpsc::Receiver<(String, Vec<NotifyMessage>)>>,
        mut file_watch_rx: Option<mpsc::Receiver<String>>,
    ) -> Result<()> {
        // Seed exit watchers from all current processes
        let mut exit_watchers: FuturesUnordered<
            Pin<Box<dyn futures::Future<Output = String> + Send>>,
        > = FuturesUnordered::new();
        {
            let jobs = jobs.read().await;
            for (name, process) in jobs.iter() {
                exit_watchers.push(Self::make_exit_watcher(name.clone(), process.job.clone()));
            }
        }

        loop {
            // Compute the next watchdog deadline across all processes
            let next_watchdog = {
                let jobs = jobs.read().await;
                jobs.values()
                    .filter(|p| !p.stopped && !p.restarting)
                    .filter_map(|p| p.watchdog_deadline())
                    .min()
            };

            tokio::select! {
                _ = &mut shutdown => {
                    info!("Shutdown requested, stopping all processes");
                    // Inline stop_all: iterate and stop each process
                    let job_names: Vec<String> = {
                        let j = jobs.read().await;
                        j.keys().cloned().collect()
                    };
                    for stop_name in job_names {
                        let mut j = jobs.write().await;
                        if let Some(mut process) = j.remove(&stop_name) {
                            process.activity.reset();
                            process.abort_subtasks();
                            if let Some((r1, r2)) = process.output_readers.take() {
                                r1.abort();
                                r2.abort();
                            }
                            drop(j);
                            process
                                .job
                                .stop_with_signal(Signal::Terminate, Duration::from_secs(5))
                                .await;
                        }
                    }
                    break;
                }

                Some(cmd) = async {
                    match command_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match cmd {
                        ProcessCommand::Restart(name) => {
                            info!("Received restart command for process: {}", name);
                            let mut jobs = jobs.write().await;
                            if let Some(process) = jobs.get_mut(&name) {
                                process.activity.reset();
                                process.activity.set_status(ProcessStatus::Restarting);

                                // Truncate log files
                                let log_dir = state_dir.join("logs");
                                let stdout_log = log_dir.join(format!("{}.stdout.log", name));
                                let stderr_log = log_dir.join(format!("{}.stderr.log", name));
                                let _ = std::fs::write(&stdout_log, "");
                                let _ = std::fs::write(&stderr_log, "");

                                let was_stopped = process.stopped;
                                process.reset_for_restart();
                                process.restarting = true;
                                process.abort_process_subtasks();

                                // Respawn TCP probe if needed
                                if !process.config.listen.is_empty()
                                    && process.config.notify.as_ref().is_none_or(|n| !n.enable)
                                    && let Some(address) = process.config.listen.iter().find_map(|spec| {
                                        if spec.kind == crate::config::ListenKind::Tcp {
                                            spec.address.clone()
                                        } else {
                                            None
                                        }
                                    }) {
                                        let ready_state = process.ready_state.clone();
                                        let probe_name = name.clone();
                                        let probe_activity = process.activity.clone();
                                        process.tcp_probe_task = Some(tokio::spawn(async move {
                                            debug!("Starting TCP probe for {} at {}", probe_name, address);
                                            loop {
                                                match tokio::net::TcpStream::connect(&address).await {
                                                    Ok(_) => {
                                                        info!("TCP probe succeeded for {} at {}", probe_name, address);
                                                        probe_activity.log("TCP probe succeeded - process ready");
                                                        probe_activity.set_status(ProcessStatus::Ready);
                                                        let _ = ready_state.send(true);
                                                        break;
                                                    }
                                                    Err(_) => {
                                                        tokio::time::sleep(Duration::from_millis(100)).await;
                                                    }
                                                }
                                            }
                                        }));
                                    }

                                // Respawn notify forwarder if needed
                                if let Some(socket) = process.notify_socket.clone() {
                                    let fwd_name = name.clone();
                                    let tx = notify_tx.clone();
                                    process.notify_forwarder_task = Some(tokio::spawn(async move {
                                        loop {
                                            match socket.recv().await {
                                                Ok(messages) => {
                                                    if tx.send((fwd_name.clone(), messages)).await.is_err() {
                                                        break;
                                                    }
                                                }
                                                Err(e) => {
                                                    debug!("Notify socket error for {}: {}", fwd_name, e);
                                                    break;
                                                }
                                            }
                                        }
                                    }));
                                }

                                if was_stopped {
                                    // Process was stopped, start fresh
                                    process.job.start().await;
                                    process.restarting = false;
                                } else {
                                    // Process is running, use try_restart to atomically restart
                                    process.job.try_restart_with_signal(
                                        Signal::Terminate,
                                        Duration::from_secs(2),
                                    ).await;
                                }

                                process.activity.set_status(ProcessStatus::Running);

                                // Push a new exit watcher
                                exit_watchers.push(Self::make_exit_watcher(
                                    name.clone(),
                                    process.job.clone(),
                                ));
                            } else {
                                warn!("Process {} not found for restart", name);
                            }
                        }
                    }
                }

                Some(name) = exit_watchers.next() => {
                    // Phase 1: Check restarting flag under write lock
                    let phase1_result = {
                        let mut jobs = jobs.write().await;
                        if let Some(process) = jobs.get_mut(&name) {
                            if process.restarting {
                                // Absorb the exit from an initiated restart
                                debug!("Absorbing stale exit for {} after initiated restart", name);
                                process.restarting = false;
                                process.status = process.initial_status();
                                // Push new exit watcher for the restarted process
                                exit_watchers.push(Self::make_exit_watcher(
                                    name.clone(),
                                    process.job.clone(),
                                ));
                                None // handled
                            } else {
                                // Extract what we need for Phase 2
                                let job = process.job.clone();
                                let policy = process.config.restart;
                                Some((job, policy))
                            }
                        } else {
                            // Process was removed (stopped externally), silently ignore
                            None
                        }
                    };

                    if let Some((job, policy)) = phase1_result {
                        // Phase 2: call run_async (no lock held)
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        let process_name = name.clone();

                        job.run_async(move |ctx| {
                            let status = if let CommandState::Finished { status, .. } = ctx.current {
                                Some(*status)
                            } else {
                                None
                            };

                            Box::new(async move {
                                if let Some(status) = status {
                                    let is_failure = !matches!(status, ProcessEnd::Success);
                                    let should_restart = match policy {
                                        RestartPolicy::Never => false,
                                        RestartPolicy::Always => true,
                                        RestartPolicy::OnFailure => is_failure,
                                    };

                                    if should_restart {
                                        debug!(
                                            "Process {} exited with {:?}, policy: {:?}",
                                            process_name, status, policy
                                        );
                                    }

                                    let _ = tx.send((should_restart, is_failure));
                                }
                            })
                        }).await;

                        // Phase 3: Act on the result under write lock
                        let mut jobs = jobs.write().await;
                        if let Some(process) = jobs.get_mut(&name) {
                            match rx.await {
                                Ok((true, is_failure)) => {
                                    // Check max restarts
                                    match process.check_exit_policy(is_failure) {
                                        ExitAction::Restart => {
                                            info!("Restarting process {} (attempt {})", name, process.restart_count);
                                            process.activity.log(format!(
                                                "Restarting (attempt {})",
                                                process.restart_count
                                            ));
                                            process.status = process.initial_status();
                                            job.start().await;
                                            exit_watchers.push(Self::make_exit_watcher(
                                                name.clone(),
                                                process.job.clone(),
                                            ));
                                        }
                                        ExitAction::GiveUp => {
                                            warn!("Process {} reached max restarts, giving up", name);
                                            process.activity.error(format!(
                                                "Max restarts ({}) reached, giving up",
                                                process.config.max_restarts.unwrap_or(0)
                                            ));
                                            process.activity.fail();
                                            process.stopped = true;
                                        }
                                        ExitAction::Stop => {
                                            debug!("Process {} will not restart", name);
                                            process.stopped = true;
                                        }
                                    }
                                }
                                Ok((false, _)) => {
                                    debug!("Process {} will not restart", name);
                                    process.stopped = true;
                                }
                                Err(_) => {
                                    debug!("Process {} exit status channel dropped", name);
                                    process.stopped = true;
                                }
                            }
                        }
                    }
                }

                Some((name, messages)) = async {
                    match notify_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    let mut jobs = jobs.write().await;
                    if let Some(process) = jobs.get_mut(&name) {
                        for msg in &messages {
                            process.handle_notify_message(msg);
                        }
                    }
                }

                Some(name) = async {
                    match file_watch_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    let mut jobs = jobs.write().await;
                    if let Some(process) = jobs.get_mut(&name) {
                        if process.restarting || process.stopped {
                            continue;
                        }
                        if let Some(cooldown) = process.file_watch_cooldown
                            && Instant::now() < cooldown
                        {
                            continue;
                        }
                        info!("File change detected for {}, restarting", name);
                        process.activity.log("File change detected, restarting");
                        process.restarting = true;
                        process.file_watch_cooldown = Some(Instant::now() + Duration::from_millis(500));
                        process.status = process.initial_status();
                        process.job.try_restart_with_signal(
                            Signal::Terminate,
                            Duration::from_secs(2),
                        ).await;
                    }
                }

                _ = async {
                    match next_watchdog {
                        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
                        None => std::future::pending().await,
                    }
                }, if next_watchdog.is_some() => {
                    let now = Instant::now();
                    let mut jobs = jobs.write().await;
                    let names: Vec<String> = jobs.keys().cloned().collect();
                    for name in names {
                        let process = jobs.get_mut(&name).unwrap();
                        if process.stopped || process.restarting {
                            continue;
                        }
                        if let Some(deadline) = process.watchdog_deadline()
                            && now >= deadline {
                                match process.handle_watchdog_timeout() {
                                    ExitAction::Restart => {
                                        process.restarting = true;
                                        process.job.try_restart_with_signal(
                                            Signal::Terminate,
                                            Duration::from_secs(2),
                                        ).await;
                                    }
                                    ExitAction::GiveUp => {
                                        process.activity.fail();
                                        process.stopped = true;
                                    }
                                    ExitAction::Stop => {
                                        process.stopped = true;
                                    }
                                }
                            }
                    }
                }

                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    let jobs = jobs.read().await;
                    if !jobs.is_empty() && jobs.values().all(|p| p.stopped) {
                        debug!("All processes stopped, exiting");
                        break;
                    }
                    if jobs.is_empty() {
                        debug!("No jobs running, exiting");
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// Run the manager event loop (keeps processes alive)
    /// This should be called when running in detached mode
    pub async fn run(self: Arc<Self>) -> Result<()> {
        use signal_hook::consts::signal::*;
        use signal_hook_tokio::Signals;

        info!("Manager event loop started");

        let mut signals = Signals::new([SIGTERM, SIGINT]).into_diagnostic()?;

        let shutdown = Box::pin(async move {
            use futures::StreamExt;
            loop {
                match signals.next().await {
                    Some(sig) if sig == SIGTERM || sig == SIGINT => break,
                    _ => continue,
                }
            }
        });

        let result = self.event_loop(shutdown, None).await;

        info!("Manager event loop stopped");
        result
    }

    /// Run the manager event loop in foreground (non-Arc version)
    ///
    /// The cancellation token allows integration with external shutdown coordination
    /// (e.g., the main app shutdown). When the token is cancelled, all processes are stopped.
    ///
    /// Note: This method relies on the cancellation token for shutdown signals.
    /// Signal handling (SIGINT/SIGTERM) is done by tokio-shutdown in the main app,
    /// which cancels the token when a signal is received.
    pub async fn run_foreground(
        &self,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<()> {
        info!("Manager event loop started (foreground)");

        let command_rx = self.command_rx.lock().await.take();
        let token = cancellation_token.clone();
        let shutdown = Box::pin(async move { token.cancelled().await });

        let result = self.event_loop(shutdown, command_rx).await;

        info!("Manager event loop stopped");
        result
    }

    /// Save the manager PID to a file
    pub fn save_manager_pid(pid_path: &PathBuf) -> Result<()> {
        let pid = std::process::id();
        std::fs::write(pid_path, pid.to_string()).into_diagnostic()?;
        debug!("Saved manager PID {} to {}", pid, pid_path.display());
        Ok(())
    }

    /// Load the manager PID from a file
    pub fn load_manager_pid(pid_path: &PathBuf) -> Result<u32> {
        let pid_str = std::fs::read_to_string(pid_path).into_diagnostic()?;
        let pid = pid_str.trim().parse::<u32>().into_diagnostic()?;
        Ok(pid)
    }

    /// Start processes from stored configs, filtering by name if specified
    async fn start_processes(
        &self,
        process_names: &[String],
        env: &HashMap<String, String>,
    ) -> Result<()> {
        // Collect configs to start (clone them to release the lock)
        let configs_to_start: Vec<ProcessConfig> = {
            let configs = self.process_configs.read().await;
            let mut names_to_start: Vec<_> = if process_names.is_empty() {
                configs.keys().cloned().collect()
            } else {
                process_names.to_vec()
            };
            names_to_start.sort();

            let mut result = Vec::new();
            for name in &names_to_start {
                let Some(process_config) = configs.get(name) else {
                    bail!("Process '{}' not found in configuration", name);
                };
                let mut config = process_config.clone();
                config.env.extend(env.clone());
                result.push(config);
            }
            result
        };

        // Create parent activity for grouping all processes
        let parent_activity = Activity::operation("Running processes").start();
        let parent_id = parent_activity.id();

        // Store the parent activity to keep it alive
        {
            let mut guard = self.processes_activity.write().await;
            *guard = Some(parent_activity);
        }

        // Start each process (lock is released, so start_command can write)
        for config in &configs_to_start {
            info!("Starting process: {}", config.name);
            self.start_command(config, Some(parent_id))
                .await
                .wrap_err_with(|| format!("Failed to start process '{}'", config.name))?;
        }

        Ok(())
    }

    /// Stop the manager daemon by sending SIGTERM
    async fn stop_manager(&self) -> Result<()> {
        let manager_pid_file = self.manager_pid_file();

        if !manager_pid_file.exists() {
            bail!("Native process manager not running (PID file not found)");
        }

        let manager_pid = Self::load_manager_pid(&manager_pid_file)?;
        let pid = Pid::from_raw(manager_pid as i32);

        info!("Stopping native process manager (PID: {})", manager_pid);

        // Send SIGTERM
        match signal::kill(pid, NixSignal::SIGTERM) {
            Ok(_) => {
                debug!("Sent SIGTERM to manager process (PID {})", pid);
            }
            Err(nix::errno::Errno::ESRCH) => {
                warn!(
                    "Manager process (PID {}) not found - removing stale PID file",
                    pid
                );
                tokio::fs::remove_file(&manager_pid_file)
                    .await
                    .into_diagnostic()
                    .wrap_err("Failed to remove stale PID file")?;
                return Ok(());
            }
            Err(e) => {
                bail!(
                    "Failed to send SIGTERM to manager process (PID {}): {}",
                    pid,
                    e
                );
            }
        }

        // Wait for shutdown with exponential backoff
        let start = std::time::Instant::now();
        let max_wait = Duration::from_secs(30);
        let mut interval = Duration::from_millis(100);
        let max_interval = Duration::from_secs(1);

        loop {
            match signal::kill(pid, None) {
                Ok(_) => {
                    if start.elapsed() >= max_wait {
                        warn!(
                            "Manager did not shut down within {} seconds, sending SIGKILL",
                            max_wait.as_secs()
                        );

                        match signal::kill(pid, NixSignal::SIGKILL) {
                            Ok(_) => info!("Sent SIGKILL to manager (PID {})", pid),
                            Err(e) => warn!("Failed to send SIGKILL: {}", e),
                        }

                        tokio::time::sleep(Duration::from_millis(100)).await;
                        break;
                    }

                    tokio::time::sleep(interval).await;
                    interval = Duration::from_secs_f64(
                        (interval.as_secs_f64() * 1.5).min(max_interval.as_secs_f64()),
                    );
                }
                Err(nix::errno::Errno::ESRCH) => {
                    debug!(
                        "Manager shut down after {:.2}s",
                        start.elapsed().as_secs_f32()
                    );
                    break;
                }
                Err(e) => {
                    warn!("Error checking manager process: {}", e);
                    break;
                }
            }
        }

        // Remove PID file
        tokio::fs::remove_file(&manager_pid_file)
            .await
            .into_diagnostic()
            .wrap_err("Failed to remove manager PID file")?;

        info!("Native process manager stopped");
        Ok(())
    }
}

#[async_trait]
impl ProcessManager for NativeProcessManager {
    async fn start(&self, options: StartOptions) -> Result<()> {
        // Check if already running
        match pid::check_pid_file(&self.manager_pid_file()).await? {
            PidStatus::Running(pid) => {
                bail!(
                    "Native process manager already running with PID {}. Stop it first with: devenv processes down",
                    pid
                );
            }
            PidStatus::NotFound | PidStatus::StaleRemoved => {}
        }

        if options.detach {
            use daemonize::{Daemonize, Outcome};

            let manager_pid_file = self.manager_pid_file();

            // Clone data needed in the daemon before daemonizing
            let state_dir = self.state_dir.clone();
            let process_configs = self.process_configs.blocking_read().clone();
            let process_names = options.processes.clone();
            let env = options.env.clone();

            // Configure and start the daemon (execute() keeps parent alive)
            let daemonize = Daemonize::new().pid_file(&manager_pid_file);

            match daemonize.execute() {
                Outcome::Parent(Ok(_)) => {
                    // Read PID from file that daemonize created
                    let pid = std::fs::read_to_string(&manager_pid_file)
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|_| "unknown".to_string());
                    info!(
                        "Native process manager started in background (PID: {})",
                        pid
                    );
                    info!("Stop with: devenv processes down");
                    return Ok(());
                }
                Outcome::Parent(Err(e)) => {
                    bail!("Failed to daemonize: {}", e);
                }
                Outcome::Child(Ok(_)) => {
                    // We're now in the daemon process
                    // Create new tokio runtime for the daemon
                    let runtime = match tokio::runtime::Runtime::new() {
                        Ok(rt) => rt,
                        Err(e) => {
                            eprintln!("Failed to create tokio runtime: {}", e);
                            std::process::exit(1);
                        }
                    };

                    // Recreate manager and run event loop
                    let result = runtime.block_on(async {
                        let manager =
                            Arc::new(NativeProcessManager::new(state_dir, process_configs)?);
                        manager.start_processes(&process_names, &env).await?;
                        manager.run().await
                    });

                    // Clean up PID file on exit
                    let _ = std::fs::remove_file(&manager_pid_file);

                    if let Err(e) = result {
                        eprintln!("Manager event loop failed: {}", e);
                        std::process::exit(1);
                    }

                    std::process::exit(0);
                }
                Outcome::Child(Err(e)) => {
                    eprintln!("Daemon child failed: {}", e);
                    std::process::exit(1);
                }
            }
        } else {
            // Start requested processes
            self.start_processes(&options.processes, &options.env)
                .await?;

            // Foreground mode - run the event loop
            info!("All processes started. Press Ctrl+C to stop.");

            // Save PID for tracking
            pid::write_pid(&self.manager_pid_file(), std::process::id()).await?;

            // Run the event loop (shutdown via cancellation token from tokio-shutdown)
            let token = options.cancellation_token.unwrap_or_default();
            let result = self.run_foreground(token).await;

            // Clean up PID file
            let _ = tokio::fs::remove_file(&self.manager_pid_file()).await;

            result
        }
    }

    async fn stop(&self) -> Result<()> {
        // Check if there's a manager daemon running
        let manager_pid_file = self.manager_pid_file();
        if manager_pid_file.exists() {
            return self.stop_manager().await;
        }

        // Otherwise just stop all local jobs
        self.stop_all().await
    }

    async fn is_running(&self) -> bool {
        matches!(
            pid::check_pid_file(&self.manager_pid_file()).await,
            Ok(PidStatus::Running(_))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_manager() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf(), HashMap::new());
        assert!(manager.is_ok());
    }

    #[tokio::test]
    async fn test_start_simple_process() {
        let temp_dir = tempfile::tempdir().unwrap();

        let config = ProcessConfig {
            name: "test-echo".to_string(),
            exec: "echo".to_string(),
            args: vec!["hello".to_string()],
            restart: RestartPolicy::Never,
            ..Default::default()
        };

        let mut configs = HashMap::new();
        configs.insert("test-echo".to_string(), config.clone());

        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf(), configs).unwrap();

        assert!(manager.start_command(&config, None).await.is_ok());
        assert_eq!(manager.list().await.len(), 1);

        // Clean up
        let _ = manager.stop_all().await;
    }
}
