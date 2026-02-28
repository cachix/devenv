use async_trait::async_trait;
use devenv_activity::{Activity, ProcessStatus};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use nix::sys::signal::{self, Signal as NixSignal};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// Commands that can be sent to control processes
#[derive(Debug, Clone)]
pub enum ProcessCommand {
    /// Restart a running process, or start a stopped process
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

use watchexec_supervisor::{
    Signal,
    job::{Job, start_job},
};

use crate::config::ProcessConfig;
use crate::pid::{self, PidStatus};
use crate::socket_activation::{ProcessSetupWrapper, activation_from_listen};
use crate::{ProcessManager, StartOptions};
use devenv_event_sources::NotifySocket;

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

/// Per-process handles shared between `JobHandle` and the supervision task.
pub struct ProcessResources {
    pub config: ProcessConfig,
    pub job: Arc<Job>,
    pub activity: Activity,
    pub notify_socket: Option<Arc<NotifySocket>>,
    pub status_tx: tokio::sync::watch::Sender<crate::supervisor_state::JobStatus>,
}

/// Handle to a managed process job
pub struct JobHandle {
    pub resources: ProcessResources,
    /// Status receiver for querying supervisor state
    pub status_rx: tokio::sync::watch::Receiver<crate::supervisor_state::JobStatus>,
    /// Supervisor task handling restarts
    pub supervisor_task: JoinHandle<()>,
    /// Output reader tasks (stdout, stderr)
    pub output_readers: Option<(JoinHandle<()>, JoinHandle<()>)>,
}

/// A managed process entry: either disabled (not running) or active (running).
enum ProcessEntry {
    /// Process has `start.enable = false`: visible in TUI but not launched.
    Disabled {
        config: ProcessConfig,
        activity: Activity,
    },
    /// Process is running under supervision.
    Active(JobHandle),
}

/// Native process manager using watchexec-supervisor
pub struct NativeProcessManager {
    processes: Arc<RwLock<HashMap<String, ProcessEntry>>>,
    state_dir: PathBuf,
    shutdown: Arc<tokio::sync::Notify>,
    /// Parent activity for grouping all processes under "Starting processes"
    processes_activity: Arc<RwLock<Option<Activity>>>,
}

impl NativeProcessManager {
    /// Create a new native process manager
    pub fn new(state_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&state_dir).into_diagnostic()?;

        Ok(Self {
            processes: Arc::new(RwLock::new(HashMap::new())),
            state_dir,
            shutdown: Arc::new(tokio::sync::Notify::new()),
            processes_activity: Arc::new(RwLock::new(None)),
        })
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

    /// Start a command with the given configuration.
    ///
    /// If `start.enable` is false, the process is registered as disabled (visible
    /// in TUI as stopped but not running) and `Ok(None)` is returned.
    pub async fn start_command(
        &self,
        config: &ProcessConfig,
        parent_id: Option<u64>,
    ) -> Result<Option<Arc<Job>>> {
        debug!("Starting command '{}': {}", config.name, config.exec);

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

        if !config.start.enable {
            activity.set_status(ProcessStatus::Disabled);
            self.processes.write().await.insert(
                config.name.clone(),
                ProcessEntry::Disabled {
                    config: config.clone(),
                    activity,
                },
            );
            info!("Registered disabled process: {}", config.name);
            return Ok(None);
        }

        self.launch(config, activity).await.map(Some)
    }

    /// Launch a process: sets up probes, sockets, supervisor, and log tailers.
    async fn launch(&self, config: &ProcessConfig, activity: Activity) -> Result<Arc<Job>> {
        activity.set_status(ProcessStatus::Running);

        // Create notify socket if configured via ready.notify
        let uses_notify = config.ready.as_ref().is_some_and(|r| r.notify);
        let notify_socket = if uses_notify {
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
        let proc_cmd = crate::command::build_command(
            &self.state_dir,
            config,
            notify_socket.as_ref().map(|s| s.path()),
            watchdog_usec,
        )?;

        // Truncate log files if they exist
        let _ = std::fs::write(&proc_cmd.stdout_log, "");
        let _ = std::fs::write(&proc_cmd.stderr_log, "");

        let (job, _task) = start_job(proc_cmd.command);
        let job = Arc::new(job);

        // Setup socket activation and/or capabilities if configured
        let has_sockets = !config.listen.is_empty();
        let has_caps = !config.linux.capabilities.is_empty();

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
                    config.name, config.linux.capabilities
                );
            }

            let capabilities = config.linux.capabilities.clone();
            job.set_spawn_hook(move |command_wrap, _ctx| {
                command_wrap.wrap(ProcessSetupWrapper::new(fds.clone(), capabilities.clone()));
            });
        }

        job.start().await;

        // Spawn file tailers to emit output to activity
        let stdout_tailer =
            crate::log_tailer::spawn_file_tailer(proc_cmd.stdout_log, activity.ref_handle(), false);
        let stderr_tailer =
            crate::log_tailer::spawn_file_tailer(proc_cmd.stderr_log, activity.ref_handle(), true);

        // Create status channel for supervisor state observation
        let initial_status = crate::supervisor_state::JobStatus {
            phase: crate::supervisor_state::SupervisorPhase::Starting,
            restart_count: 0,
        };
        let (status_tx, status_rx) = tokio::sync::watch::channel(initial_status);

        // If no readiness mechanism is configured, mark process as immediately ready
        let has_notify = config.ready.as_ref().is_some_and(|r| r.notify);
        let has_ready_config = config.ready.is_some();
        let has_tcp_probe = (!config.listen.is_empty() || !config.ports.is_empty()) && !has_notify;
        if !has_notify && !has_ready_config && !has_tcp_probe {
            let _ = status_tx.send(crate::supervisor_state::JobStatus {
                phase: crate::supervisor_state::SupervisorPhase::Ready,
                restart_count: 0,
            });
        }

        let resources = ProcessResources {
            config: config.clone(),
            job: job.clone(),
            activity,
            notify_socket,
            status_tx,
        };

        // Spawn supervision task
        let supervisor_task =
            crate::supervisor::spawn_supervisor(&resources, self.shutdown.clone());

        // Store the job handle
        let mut processes = self.processes.write().await;
        processes.insert(
            config.name.clone(),
            ProcessEntry::Active(JobHandle {
                resources,
                status_rx,
                supervisor_task,
                output_readers: Some((stdout_tailer, stderr_tailer)),
            }),
        );

        info!("Command '{}' started", config.name);
        Ok(job)
    }

    /// Stop a process by name
    pub async fn stop(&self, name: &str) -> Result<()> {
        let mut processes = self.processes.write().await;

        match processes.remove(name) {
            Some(ProcessEntry::Active(handle)) => {
                debug!("Stopping process: {}", name);

                // Mark as stopped so the TUI shows the correct status
                // before the Complete event arrives from Activity::drop.
                handle.resources.activity.set_status(ProcessStatus::Stopped);
                handle.resources.activity.reset();

                // Abort the supervisor task first to prevent restarts
                handle.supervisor_task.abort();

                // Abort output reader tasks
                if let Some((stdout_reader, stderr_reader)) = handle.output_readers {
                    stdout_reader.abort();
                    stderr_reader.abort();
                }

                // Send terminate signal with grace period
                handle
                    .resources
                    .job
                    .stop_with_signal(Signal::Terminate, Duration::from_secs(5))
                    .await;

                info!("Process {} stopped", name);
                Ok(())
            }
            Some(entry @ ProcessEntry::Disabled { .. }) => {
                // Put it back; disabled processes are not stoppable
                processes.insert(name.to_string(), entry);
                bail!("Process {} is disabled, cannot stop", name)
            }
            None => bail!("Process {} not found", name),
        }
    }

    /// Signal all supervisors to shut down gracefully.
    ///
    /// This wakes the supervisor loops so they exit before we abort their tasks.
    pub fn shutdown_supervisors(&self) {
        self.shutdown.notify_waiters();
    }

    /// Stop all processes and clear disabled entries
    pub async fn stop_all(&self) -> Result<()> {
        // Signal supervisors first so they exit gracefully
        self.shutdown_supervisors();

        let processes = self.processes.read().await;
        let active_names: Vec<String> = processes
            .iter()
            .filter_map(|(name, entry)| match entry {
                ProcessEntry::Active(_) => Some(name.clone()),
                ProcessEntry::Disabled { .. } => None,
            })
            .collect();
        drop(processes); // Release the read lock

        for name in active_names {
            let _ = self.stop(&name).await; // Continue even if one fails
        }

        // Clear disabled processes (their activities complete on drop)
        self.processes
            .write()
            .await
            .retain(|_, entry| !matches!(entry, ProcessEntry::Disabled { .. }));

        Ok(())
    }

    /// Restart a process by name
    ///
    /// This resets the restart count and activity state, respawns the supervision
    /// task if it exited (e.g., due to max restarts), and restarts the underlying job.
    pub async fn restart(&self, name: &str) -> Result<()> {
        let mut processes = self.processes.write().await;
        let handle = match processes.get_mut(name) {
            Some(ProcessEntry::Active(h)) => h,
            Some(ProcessEntry::Disabled { .. }) => {
                bail!("Process {} is disabled, use start_disabled", name)
            }
            None => bail!("Process {} not running", name),
        };

        // Reset activity state (unfail it) and set status to restarting
        handle.resources.activity.reset();
        handle
            .resources
            .activity
            .set_status(ProcessStatus::Restarting);

        // Truncate log files and restart output tailers
        let (stdout_log, stderr_log) = crate::command::log_paths(&self.state_dir, name);
        let _ = std::fs::write(&stdout_log, "");
        let _ = std::fs::write(&stderr_log, "");

        if let Some((stdout_reader, stderr_reader)) = handle.output_readers.take() {
            stdout_reader.abort();
            stderr_reader.abort();
        }
        handle.output_readers = Some((
            crate::log_tailer::spawn_file_tailer(
                stdout_log,
                handle.resources.activity.ref_handle(),
                false,
            ),
            crate::log_tailer::spawn_file_tailer(
                stderr_log,
                handle.resources.activity.ref_handle(),
                true,
            ),
        ));

        // Check if supervisor task has exited (e.g., due to max restarts)
        if handle.supervisor_task.is_finished() {
            // Supervisor has exited — start fresh with new supervision.
            // Order matters: start job first, then spawn supervisor (like start_command does).
            // This gives the process a fresh restart quota (restart_count = 0).
            info!(
                "Supervisor for {} has exited, starting fresh with new supervision",
                name
            );
            handle.resources.job.start().await;
            handle.supervisor_task =
                crate::supervisor::spawn_supervisor(&handle.resources, self.shutdown.clone());
        } else {
            // Supervisor is still running — just restart the job.
            // The existing supervisor will continue monitoring with its current restart_count.
            handle
                .resources
                .job
                .restart_with_signal(Signal::Terminate, Duration::from_secs(2))
                .await;
        }

        // The supervisor will update the status via status_tx once the
        // process is actually ready.
        handle.resources.activity.set_status(ProcessStatus::Running);

        info!("Process {} restarted", name);
        Ok(())
    }

    /// Start a previously disabled process, reusing its existing TUI activity.
    pub async fn start_disabled(&self, name: &str) -> Result<Arc<Job>> {
        let removed = self.processes.write().await.remove(name);
        let (config, activity) = match removed {
            Some(ProcessEntry::Disabled { config, activity }) => (config, activity),
            Some(entry) => {
                // Put it back
                self.processes.write().await.insert(name.to_string(), entry);
                bail!("Process {} is not disabled", name);
            }
            None => bail!("Process {} not found", name),
        };

        // Reset the activity so it no longer shows as stopped
        activity.reset();

        info!("Starting disabled process: {}", name);
        // Move the activity into launch (not clone) so the original is not
        // dropped — Activity::drop sends Process::Complete which would
        // immediately mark the process as stopped in the TUI.
        self.launch(&config, activity).await
    }

    /// Get list of running processes
    pub async fn list(&self) -> Vec<String> {
        let processes = self.processes.read().await;
        processes
            .iter()
            .filter_map(|(name, entry)| match entry {
                ProcessEntry::Active(_) => Some(name.clone()),
                ProcessEntry::Disabled { .. } => None,
            })
            .collect()
    }

    /// Wait for a process to become ready, avoiding missed early readiness signals.
    ///
    /// Respects the provided cancellation token so that shutdown (e.g. SIGINT) can
    /// interrupt the wait instead of blocking indefinitely.
    pub async fn wait_ready(&self, name: &str, cancel: &CancellationToken) -> Result<()> {
        let mut status_rx = {
            let processes = self.processes.read().await;
            match processes.get(name) {
                Some(ProcessEntry::Active(handle)) => handle.status_rx.clone(),
                Some(ProcessEntry::Disabled { .. }) => {
                    bail!("Process {} is disabled", name)
                }
                None => bail!("Process {} not found", name),
            }
        };

        if status_rx.borrow().is_ready() {
            return Ok(());
        }

        loop {
            tokio::select! {
                changed = status_rx.changed() => {
                    match changed {
                        Ok(()) => {
                            if status_rx.borrow().is_ready() {
                                return Ok(());
                            }
                        }
                        Err(_) => bail!("Process {} ready state channel closed", name),
                    }
                }
                _ = cancel.cancelled() => {
                    bail!("Process {} readiness wait cancelled", name);
                }
            }
        }
    }

    /// Query the current state of a process.
    pub async fn job_state(&self, name: &str) -> Option<crate::supervisor_state::JobStatus> {
        let processes = self.processes.read().await;
        match processes.get(name) {
            Some(ProcessEntry::Active(handle)) => Some(handle.status_rx.borrow().clone()),
            _ => None,
        }
    }

    /// Start the API socket server for external queries (e.g., `devenv processes wait`).
    ///
    /// Listens on `state_dir/native.sock` using newline-delimited JSON (`ApiRequest`/`ApiResponse`).
    /// Must be called after all initial processes have been registered in `jobs`.
    pub fn start_api_server(&self) -> Result<()> {
        let sock_path = self.api_socket_path();
        let _ = std::fs::remove_file(&sock_path);
        let processes = self.processes.clone();

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
                        let processes = processes.clone();
                        tokio::spawn(Self::handle_api_client(stream, processes));
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
        processes: Arc<RwLock<HashMap<String, ProcessEntry>>>,
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
                let receivers: Vec<(
                    String,
                    tokio::sync::watch::Receiver<crate::supervisor_state::JobStatus>,
                )> = {
                    let processes = processes.read().await;
                    processes
                        .iter()
                        .filter_map(|(name, entry)| match entry {
                            ProcessEntry::Active(handle) => {
                                Some((name.clone(), handle.status_rx.clone()))
                            }
                            ProcessEntry::Disabled { .. } => None,
                        })
                        .collect()
                };

                for (name, mut rx) in receivers {
                    if rx.borrow().is_ready() {
                        continue;
                    }
                    debug!("API: waiting for process {} to become ready", name);
                    while rx.changed().await.is_ok() {
                        if rx.borrow().is_ready() {
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
                    if self.processes.read().await.is_empty() {
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
        mut command_rx: Option<mpsc::Receiver<ProcessCommand>>,
    ) -> Result<()> {
        info!("Manager event loop started (foreground)");

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
                            let is_disabled = matches!(
                                self.processes.read().await.get(&name),
                                Some(ProcessEntry::Disabled { .. })
                            );
                            if is_disabled {
                                info!("Starting disabled process: {}", name);
                                if let Err(e) = self.start_disabled(&name).await {
                                    warn!("Failed to start disabled process {}: {}", name, e);
                                }
                            } else {
                                info!("Restarting process: {}", name);
                                if let Err(e) = self.restart(&name).await {
                                    warn!("Failed to restart process {}: {}", name, e);
                                }
                            }
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if self.processes.read().await.is_empty() {
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
    pub fn save_manager_pid(pid_path: &Path) -> Result<()> {
        let pid = std::process::id();
        std::fs::write(pid_path, pid.to_string()).into_diagnostic()?;
        debug!("Saved manager PID {} to {}", pid, pid_path.display());
        Ok(())
    }

    /// Load the manager PID from a file
    pub fn load_manager_pid(pid_path: &Path) -> Result<u32> {
        let pid_str = std::fs::read_to_string(pid_path).into_diagnostic()?;
        let pid = pid_str.trim().parse::<u32>().into_diagnostic()?;
        Ok(pid)
    }

    /// Start processes from the given configs, filtering by name if specified
    async fn start_processes(
        &self,
        configs: &HashMap<String, ProcessConfig>,
        process_names: &[String],
        env: &HashMap<String, String>,
    ) -> Result<()> {
        let mut names_to_start: Vec<_> = if process_names.is_empty() {
            configs.keys().cloned().collect()
        } else {
            process_names.to_vec()
        };
        names_to_start.sort();

        let configs_to_start: Vec<ProcessConfig> = {
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
            let process_configs = options.process_configs.clone();
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
                        let manager = Arc::new(NativeProcessManager::new(state_dir)?);
                        manager
                            .start_processes(&process_configs, &process_names, &env)
                            .await?;
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
            self.start_processes(&options.process_configs, &options.processes, &options.env)
                .await?;

            // Foreground mode - run the event loop
            info!("All processes started. Press Ctrl+C to stop.");

            // Save PID for tracking
            pid::write_pid(&self.manager_pid_file(), std::process::id()).await?;

            // Run the event loop (shutdown via cancellation token from tokio-shutdown)
            let token = options.cancellation_token.unwrap_or_default();
            let result = self.run_foreground(token, None).await;

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
        // Signal supervisors to exit so they don't keep running after the manager is gone
        self.shutdown_supervisors();
        let _ = std::fs::remove_file(self.api_socket_path());
        let _ = std::fs::remove_file(self.manager_pid_file());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RestartPolicy;

    #[tokio::test]
    async fn test_create_manager() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf());
        assert!(manager.is_ok());
    }

    #[tokio::test]
    async fn test_start_simple_process() {
        let temp_dir = tempfile::tempdir().unwrap();

        let config = ProcessConfig {
            name: "test-echo".to_string(),
            exec: "echo".to_string(),
            args: vec!["hello".to_string()],
            restart: crate::config::RestartConfig {
                on: RestartPolicy::Never,
                max: Some(5),
                window: None,
            },
            ..Default::default()
        };

        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();

        assert!(manager.start_command(&config, None).await.is_ok());
        assert_eq!(manager.list().await.len(), 1);

        // Clean up
        let _ = manager.stop_all().await;
    }
}
