use async_trait::async_trait;
use devenv_activity::{Activity, ProcessStatus};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use nix::sys::signal::{self, Signal as NixSignal};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

/// Request sent by a client to the native manager API socket.
///
/// Protocol: newline-delimited JSON over a Unix stream socket.
/// The client sends one `ApiRequest` per line, the server responds with one `ApiResponse`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum ApiRequest {
    /// Block until every managed process is ready, then respond.
    Wait,
}

/// Response sent by the native manager API socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ApiResponse {
    /// All processes are ready.
    Ready,
    /// An error occurred.
    Error { message: String },
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

/// Handle to a managed process job
pub struct JobHandle {
    /// The watchexec job for process control
    pub job: Arc<Job>,
    /// Supervisor task handling restarts
    pub supervisor_task: JoinHandle<()>,
    /// Activity for tracking process lifecycle
    pub activity: Activity,
    /// Output reader tasks (stdout, stderr)
    pub output_readers: Option<(JoinHandle<()>, JoinHandle<()>)>,
    /// Notify socket for systemd-style notifications (owned here to keep alive)
    pub notify_socket: Option<Arc<NotifySocket>>,
    /// Ready state for signaling when process becomes ready (READY=1 or TCP probe)
    pub ready_state: tokio::sync::watch::Sender<bool>,
}

/// Native process manager using watchexec-supervisor
pub struct NativeProcessManager {
    jobs: Arc<RwLock<HashMap<String, JobHandle>>>,
    state_dir: PathBuf,
    shutdown: Arc<tokio::sync::Notify>,
    /// Process configurations (populated when processes are started)
    process_configs: RwLock<HashMap<String, ProcessConfig>>,
    /// Command receiver for process control (restart, etc.)
    command_rx: Arc<tokio::sync::Mutex<Option<mpsc::Receiver<ProcessCommand>>>>,
    /// Parent activity for grouping all processes under "Starting processes"
    processes_activity: Arc<RwLock<Option<Activity>>>,
}

impl NativeProcessManager {
    /// Create a new native process manager
    pub fn new(
        state_dir: PathBuf,
        process_configs: HashMap<String, ProcessConfig>,
    ) -> Result<Self> {
        std::fs::create_dir_all(&state_dir).into_diagnostic()?;

        Ok(Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            state_dir,
            shutdown: Arc::new(tokio::sync::Notify::new()),
            process_configs: RwLock::new(process_configs),
            command_rx: Arc::new(tokio::sync::Mutex::new(None)),
            processes_activity: Arc::new(RwLock::new(None)),
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

    /// Path to the API socket
    pub fn api_socket_path(&self) -> PathBuf {
        self.state_dir.join("native.sock")
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

        // If no readiness mechanism is configured, mark process as immediately ready
        let has_notify = config.notify.as_ref().is_some_and(|n| n.enable);
        let has_tcp_probe = !config.listen.is_empty() && !has_notify;
        if !has_notify && !has_tcp_probe {
            let _ = ready_state.send(true);
        }

        // Spawn supervision task
        let supervisor_task = self.spawn_supervisor(
            config.clone(),
            job.clone(),
            activity.clone(),
            notify_socket.clone(),
            ready_state.clone(),
        );

        // Store the job handle
        let job_clone = job.clone();
        let mut jobs = self.jobs.write().await;
        jobs.insert(
            config.name.clone(),
            JobHandle {
                job,
                supervisor_task,
                activity,
                output_readers: Some((stdout_tailer, stderr_tailer)),
                notify_socket,
                ready_state,
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

    /// Spawn a supervision task that monitors the job and handles restarts
    fn spawn_supervisor(
        &self,
        config: ProcessConfig,
        job: Arc<Job>,
        activity: Activity,
        notify_socket: Option<Arc<NotifySocket>>,
        ready_state: tokio::sync::watch::Sender<bool>,
    ) -> JoinHandle<()> {
        let shutdown = self.shutdown.clone();
        let name = config.name.clone();

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

        // Check if we need TCP probe for readiness (listen sockets or allocated ports, without notify)
        let tcp_probe_address = if config.notify.as_ref().is_none_or(|n| !n.enable) {
            // First try listen sockets
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
                // Fall back to the first allocated port
                .or_else(|| {
                    config
                        .ports
                        .values()
                        .next()
                        .map(|port| format!("127.0.0.1:{}", port))
                })
        } else {
            None
        };

        tokio::spawn(async move {
            let mut restart_count = 0usize;
            let mut ready_signaled = false;

            // Watchdog state
            let mut is_ready = !watchdog_require_ready; // If require_ready is false, start as ready
            let mut watchdog_deadline: Option<Instant> = if watchdog_timeout.is_some() && is_ready {
                Some(Instant::now() + watchdog_timeout.unwrap())
            } else {
                None
            };

            // Spawn TCP probe task if needed
            let _tcp_probe_task = if let Some(address) = tcp_probe_address {
                let ready_state = ready_state.clone();
                let probe_name = name.clone();
                let probe_activity = activity.clone();
                Some(tokio::spawn(async move {
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
                }))
            } else {
                None
            };

            // Set up file watcher if watch paths are configured
            let (watch_tx, mut watch_rx) = mpsc::channel::<()>(1);
            let _watcher_task = if !config.watch.paths.is_empty() {
                // Canonicalize watch paths to resolve symlinks. On macOS,
                // /tmp -> /private/tmp and /var -> /private/var; FSEvents
                // reports events using resolved paths, so the watched paths
                // must match for watchexec to attach path tags to events.
                let paths: Vec<PathBuf> = config
                    .watch
                    .paths
                    .iter()
                    .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
                    .collect();
                let extensions = config.watch.extensions.clone();
                let ignore = config.watch.ignore.clone();
                let watch_name = name.clone();
                let tx = watch_tx.clone();

                Some(tokio::spawn(async move {
                    // Build ignore patterns for GlobsetFilterer
                    // Format: (pattern, optional_base_path)
                    let ignores: Vec<(String, Option<PathBuf>)> = ignore
                        .iter()
                        .map(|pattern| {
                            // Support both "**/pattern" and "pattern" styles
                            let glob_pattern = if pattern.contains('/') || pattern.starts_with("**")
                            {
                                pattern.clone()
                            } else {
                                format!("**/{}", pattern)
                            };
                            (glob_pattern, None)
                        })
                        .collect();

                    // Get origin path (first watch path or current dir)
                    let origin = paths.first().cloned().unwrap_or_else(|| PathBuf::from("."));

                    // Create the filterer
                    let filterer = match GlobsetFilterer::new(
                        &origin,
                        std::iter::empty::<(String, Option<PathBuf>)>(), // no filters (allow all)
                        ignores,
                        std::iter::empty::<PathBuf>(), // no whitelist
                        std::iter::empty(),            // no ignore files
                        extensions.iter().map(|e| std::ffi::OsString::from(e)), // extension filter
                    )
                    .await
                    {
                        Ok(f) => Arc::new(f),
                        Err(e) => {
                            warn!("Failed to create filterer for {}: {}", watch_name, e);
                            return;
                        }
                    };

                    let tx = tx;
                    let wx = match Watchexec::new(move |action| {
                        // Events are already filtered by the filterer
                        if action.events.iter().any(|e| e.paths().next().is_some()) {
                            let _ = tx.try_send(());
                        }
                        action
                    }) {
                        Ok(wx) => wx,
                        Err(e) => {
                            warn!("Failed to create file watcher for {}: {}", watch_name, e);
                            return;
                        }
                    };

                    // Configure paths to watch
                    wx.config.pathset(paths.iter().map(|p| p.as_path()));

                    // Set the filterer
                    wx.config.filterer(filterer);

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

            loop {
                tokio::select! {
                    _ = shutdown.notified() => {
                        debug!("Shutdown requested for {}", name);
                        break;
                    }
                    _ = watch_rx.recv() => {
                        // File change detected, restart the process
                        info!("File change detected for {}, restarting", name);
                        activity.log("File change detected, restarting");
                        job.stop_with_signal(Signal::Terminate, Duration::from_secs(2)).await;
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        job.start().await;
                        // Reset watchdog state on restart
                        is_ready = !watchdog_require_ready;
                        watchdog_deadline = if watchdog_timeout.is_some() && is_ready {
                            Some(Instant::now() + watchdog_timeout.unwrap())
                        } else {
                            None
                        };
                    }
                    // Handle notify socket messages
                    result = async {
                        match &notify_socket {
                            Some(socket) => socket.recv().await,
                            None => std::future::pending().await,
                        }
                    } => {
                        if let Ok(messages) = result {
                            for msg in messages {
                                match msg {
                                    NotifyMessage::Ready => {
                                        info!("Process {} signaled ready", name);
                                        activity.log("Process signaled ready");
                                        activity.set_status(ProcessStatus::Ready);
                                        // Signal waiting tasks that process is ready
                                        if !ready_signaled {
                                            ready_signaled = true;
                                            let _ = ready_state.send(true);
                                        }
                                        // Start watchdog timer now if configured
                                        if let Some(timeout) = watchdog_timeout {
                                            watchdog_deadline = Some(Instant::now() + timeout);
                                        }
                                    }
                                    NotifyMessage::Watchdog => {
                                        debug!("Watchdog ping from {}", name);
                                        if let Some(timeout) = watchdog_timeout {
                                            watchdog_deadline = Some(Instant::now() + timeout);
                                        }
                                    }
                                    NotifyMessage::Status(status) => {
                                        debug!("Status from {}: {}", name, status);
                                        activity.log(&format!("Status: {}", status));
                                    }
                                    NotifyMessage::Stopping => {
                                        debug!("Process {} signaled stopping", name);
                                        activity.log("Process signaled stopping");
                                    }
                                    NotifyMessage::Reloading => {
                                        debug!("Process {} signaled reloading", name);
                                        activity.log("Process reloading configuration");
                                    }
                                    NotifyMessage::WatchdogTrigger => {
                                        debug!("Watchdog trigger from {}", name);
                                    }
                                    NotifyMessage::ExtendTimeout { usec } => {
                                        debug!("Extend timeout from {}: {} usec", name, usec);
                                    }
                                    NotifyMessage::Unknown(s) => {
                                        debug!("Unknown notify message from {}: {}", name, s);
                                    }
                                }
                            }
                        }
                    }
                    // Handle watchdog timeout
                    _ = async {
                        match watchdog_deadline {
                            Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
                            None => std::future::pending().await,
                        }
                    }, if watchdog_deadline.is_some() => {
                        warn!("Watchdog timeout for process {}", name);
                        activity.error("Watchdog timeout - no heartbeat received");

                        // Check max restarts limit before restarting
                        if let Some(max) = config.max_restarts {
                            if restart_count >= max {
                                warn!(
                                    "Process {} reached max restarts ({}) after watchdog timeout, giving up",
                                    name, max
                                );
                                activity.error(format!("Max restarts ({}) reached, giving up", max));
                                activity.fail();
                                break;
                            }
                        }

                        // Restart on watchdog timeout
                        restart_count += 1;
                        info!("Restarting process {} due to watchdog timeout (attempt {})", name, restart_count);
                        activity.log(format!("Restarting due to watchdog timeout (attempt {})", restart_count));
                        job.restart_with_signal(Signal::Terminate, Duration::from_secs(2)).await;

                        // Reset watchdog state for new instance
                        is_ready = !watchdog_require_ready;
                        watchdog_deadline = if watchdog_timeout.is_some() && is_ready {
                            Some(Instant::now() + watchdog_timeout.unwrap())
                        } else {
                            None
                        };
                    }
                    _ = job.to_wait() => {
                        // Process ended, check if we should restart
                        let policy = config.restart;
                        let max_restarts = config.max_restarts;
                        let process_name = name.clone();

                        // Use a channel to get the restart decision from run_async
                        let (tx, rx) = tokio::sync::oneshot::channel();

                        job.run_async(move |ctx| {
                            // Extract status before async block (lifetime constraint)
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

                                    let _ = tx.send(should_restart);
                                }
                            })
                        }).await;

                        // Check restart decision
                        match rx.await {
                            Ok(true) => {
                                // Check max restarts limit
                                if let Some(max) = max_restarts
                                    && restart_count >= max
                                {
                                    warn!(
                                        "Process {} reached max restarts ({}), giving up",
                                        name, max
                                    );
                                    activity.error(format!("Max restarts ({}) reached, giving up", max));
                                    activity.fail();
                                    break;
                                }

                                restart_count += 1;
                                info!("Restarting process {} (attempt {})", name, restart_count);
                                activity.log(format!("Restarting (attempt {})", restart_count));
                                job.start().await;
                                // Reset watchdog state for new instance
                                is_ready = !watchdog_require_ready;
                                watchdog_deadline = if watchdog_timeout.is_some() && is_ready {
                                    Some(Instant::now() + watchdog_timeout.unwrap())
                                } else {
                                    None
                                };
                            }
                            _ => {
                                debug!("Process {} will not restart", name);
                                break;
                            }
                        }
                    }
                }
            }

            debug!("Supervision task for {} exiting", name);
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

        if let Some(handle) = jobs.remove(name) {
            debug!("Stopping process: {}", name);

            // Stopping a process intentionally is not a failure
            handle.activity.reset();

            // Abort the supervisor task first to prevent restarts
            handle.supervisor_task.abort();

            // Abort output reader tasks
            if let Some((stdout_reader, stderr_reader)) = handle.output_readers {
                stdout_reader.abort();
                stderr_reader.abort();
            }

            // Send terminate signal with grace period
            handle
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

    /// Restart a process by name
    ///
    /// This resets the restart count and activity state, respawns the supervision
    /// task if it exited (e.g., due to max restarts), and restarts the underlying job.
    pub async fn restart(&self, name: &str) -> Result<()> {
        // Get the process config - needed if we need to respawn the supervisor
        let config = {
            let configs = self.process_configs.read().await;
            configs
                .get(name)
                .ok_or_else(|| miette::miette!("Process {} not found in configuration", name))?
                .clone()
        };

        let mut jobs = self.jobs.write().await;
        let handle = jobs
            .get_mut(name)
            .ok_or_else(|| miette::miette!("Process {} not running", name))?;

        // Reset activity state (unfail it) and set status to restarting
        handle.activity.reset();
        handle.activity.set_status(ProcessStatus::Restarting);

        // Get log file paths and truncate them
        let log_dir = self.state_dir.join("logs");
        let stdout_log = log_dir.join(format!("{}.stdout.log", name));
        let stderr_log = log_dir.join(format!("{}.stderr.log", name));
        let _ = std::fs::write(&stdout_log, "");
        let _ = std::fs::write(&stderr_log, "");

        // Check if supervisor task has exited (e.g., due to max restarts)
        if handle.supervisor_task.is_finished() {
            // Supervisor has exited - need to start fresh with new supervision.
            // Order matters: start job first, then spawn supervisor (like start_command does).
            // This gives the process a fresh restart quota (restart_count = 0).
            info!(
                "Supervisor for {} has exited, starting fresh with new supervision",
                name
            );
            handle.job.start().await;
            handle.supervisor_task = self.spawn_supervisor(
                config,
                handle.job.clone(),
                handle.activity.clone(),
                handle.notify_socket.clone(),
                handle.ready_state.clone(),
            );
        } else {
            // Supervisor is still running - just restart the job.
            // The existing supervisor will continue monitoring with its current restart_count.
            handle
                .job
                .restart_with_signal(Signal::Terminate, Duration::from_secs(2))
                .await;
        }

        // Set status back to running
        handle.activity.set_status(ProcessStatus::Running);

        info!("Process {} restarted", name);
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

    /// Start the API socket server for external queries (e.g., `devenv processes wait`).
    ///
    /// Listens on `state_dir/native.sock` using newline-delimited JSON (`ApiRequest`/`ApiResponse`).
    /// Must be called after all initial processes have been registered in `jobs`.
    pub fn start_api_server(&self) -> Result<()> {
        let sock_path = self.api_socket_path();
        let _ = std::fs::remove_file(&sock_path);
        let jobs = self.jobs.clone();

        let listener = std::os::unix::net::UnixListener::bind(&sock_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to bind API socket at {}", sock_path.display()))?;
        listener.set_nonblocking(true).into_diagnostic()?;
        let listener = tokio::net::UnixListener::from_std(listener).into_diagnostic()?;
        info!("API server listening on {}", sock_path.display());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let jobs = jobs.clone();
                        tokio::spawn(Self::handle_api_client(stream, jobs));
                    }
                    Err(e) => {
                        warn!("API accept error: {}", e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });

        Ok(())
    }

    /// Handle a single API client connection.
    async fn handle_api_client(
        stream: tokio::net::UnixStream,
        jobs: Arc<RwLock<HashMap<String, JobHandle>>>,
    ) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        if reader.read_line(&mut line).await.is_err() {
            return;
        }

        let response = match serde_json::from_str::<ApiRequest>(&line) {
            Ok(ApiRequest::Wait) => {
                // Subscribe to all process ready channels.
                // If a process is already ready (borrow() == true), it is skipped
                // immediately — this handles clients connecting after readiness.
                let receivers: Vec<(String, tokio::sync::watch::Receiver<bool>)> = {
                    let jobs = jobs.read().await;
                    jobs.iter()
                        .map(|(name, handle)| (name.clone(), handle.ready_state.subscribe()))
                        .collect()
                };

                for (name, mut rx) in receivers {
                    if *rx.borrow() {
                        continue;
                    }
                    debug!("API: waiting for process {} to become ready", name);
                    while rx.changed().await.is_ok() {
                        if *rx.borrow() {
                            break;
                        }
                    }
                }

                ApiResponse::Ready
            }
            Err(e) => ApiResponse::Error {
                message: format!("invalid request: {}", e),
            },
        };

        if let Ok(mut json) = serde_json::to_vec(&response) {
            json.push(b'\n');
            let _ = writer.write_all(&json).await;
        }
    }

    /// Connect to a running native manager's API socket and send a request.
    pub async fn api_request(socket_path: &Path, request: &ApiRequest) -> Result<ApiResponse> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let mut stream = tokio::net::UnixStream::connect(socket_path)
            .await
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "Failed to connect to native manager at {}",
                    socket_path.display()
                )
            })?;

        let mut request_json = serde_json::to_vec(request).into_diagnostic()?;
        request_json.push(b'\n');
        stream
            .write_all(&request_json)
            .await
            .into_diagnostic()
            .wrap_err("Failed to send request to native manager")?;

        let mut reader = BufReader::new(&mut stream);
        let mut response = String::new();
        reader
            .read_line(&mut response)
            .await
            .into_diagnostic()
            .wrap_err("Failed to read response from native manager")?;

        serde_json::from_str(&response)
            .into_diagnostic()
            .wrap_err("Failed to parse response from native manager")
    }

    /// Connect to a running native manager's API socket and wait for all processes to be ready.
    pub async fn wait_for_ready(socket_path: &Path) -> Result<()> {
        match Self::api_request(socket_path, &ApiRequest::Wait).await? {
            ApiResponse::Ready => Ok(()),
            ApiResponse::Error { message } => bail!("Native manager error: {}", message),
        }
    }

    /// Run the manager event loop (keeps processes alive)
    /// This should be called when running in detached mode
    pub async fn run(self: Arc<Self>) -> Result<()> {
        use futures::stream::StreamExt;
        use signal_hook::consts::signal::*;
        use signal_hook_tokio::Signals;

        info!("Manager event loop started");

        // Set up signal handling for graceful shutdown
        let signals = Signals::new([SIGTERM, SIGINT]).into_diagnostic()?;
        let mut signals = signals.fuse();

        loop {
            tokio::select! {
                Some(signal) = signals.next() => {
                    match signal {
                        SIGTERM | SIGINT => {
                            info!("Received shutdown signal, stopping all processes");
                            self.stop_all().await?;
                            break;
                        }
                        _ => {}
                    }
                }
                // Add a small sleep to avoid busy loop
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    // Check if all jobs are still alive
                    let jobs = self.jobs.read().await;
                    if jobs.is_empty() {
                        debug!("No jobs running, exiting");
                        break;
                    }
                }
            }
        }

        info!("Manager event loop stopped");
        Ok(())
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

        // Take the command receiver from the struct
        let mut command_rx = self.command_rx.lock().await.take();

        loop {
            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    info!("Shutdown requested, stopping all processes");
                    self.stop_all().await?;
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
                            if let Err(e) = self.restart(&name).await {
                                warn!("Failed to restart process {}: {}", name, e);
                            }
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    let jobs = self.jobs.read().await;
                    if jobs.is_empty() {
                        debug!("No jobs running, exiting");
                        break;
                    }
                }
            }
        }

        info!("Manager event loop stopped");
        Ok(())
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

        // Start the API socket server for external queries
        self.start_api_server()?;

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
            let token = options
                .cancellation_token
                .unwrap_or_else(tokio_util::sync::CancellationToken::new);
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

impl Drop for NativeProcessManager {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.api_socket_path());
        let _ = std::fs::remove_file(self.manager_pid_file());
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
