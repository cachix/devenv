use async_trait::async_trait;
use devenv_activity::{Activity, ProcessStatus};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use nix::sys::signal::{self, Signal as NixSignal};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
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
    pub stderr_log: PathBuf,
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

/// Lifecycle phase of a managed process.
///
/// Shared between the process manager and the task system to avoid
/// duplicate enum definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessPhase {
    /// Process has `start.enable = false`; not yet launched.
    NotStarted,
    /// Registered, waiting for dependencies before starting.
    Waiting,
    /// Launched, readiness not yet confirmed.
    Starting,
    /// Readiness probe passed.
    Ready,
    /// Supervisor gave up (crash loop).
    GaveUp,
}

impl From<crate::supervisor_state::SupervisorPhase> for ProcessPhase {
    fn from(phase: crate::supervisor_state::SupervisorPhase) -> Self {
        match phase {
            crate::supervisor_state::SupervisorPhase::Starting => Self::Starting,
            crate::supervisor_state::SupervisorPhase::Ready => Self::Ready,
            crate::supervisor_state::SupervisorPhase::GaveUp => Self::GaveUp,
        }
    }
}

/// Collect the names of all active (supervised) processes from the map.
fn active_names(processes: &HashMap<String, ProcessEntry>) -> Vec<String> {
    processes
        .iter()
        .filter_map(|(name, entry)| match entry {
            ProcessEntry::Active(_) => Some(name.clone()),
            ProcessEntry::NotStarted { .. } | ProcessEntry::Waiting { .. } => None,
        })
        .collect()
}

/// A managed process entry: not started, waiting for dependencies, or active.
enum ProcessEntry {
    /// Process has `start.enable = false`: visible in TUI but not yet launched.
    NotStarted {
        config: ProcessConfig,
        activity: Activity,
    },
    /// Process is waiting for dependencies before starting.
    Waiting {
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
    shutdown: CancellationToken,
    /// Parent activity for grouping all processes under "Starting processes"
    processes_activity: Arc<RwLock<Option<Activity>>>,
    /// Optional notify handle fired when a process lifecycle changes (e.g. not-started
    /// process is manually started). The task system uses this to re-check dependencies.
    task_notify: Option<Arc<Notify>>,
}

/// Build a human-readable description of the readiness probe for TUI display.
fn probe_description(config: &ProcessConfig) -> Option<String> {
    let ready = config.ready.as_ref()?;
    if ready.exec.is_some() {
        return Some("exec".to_string());
    }
    if let Some(http) = &ready.http
        && let Some(get) = &http.get
    {
        return Some(format!("http: {}:{}{}", get.host, get.port, get.path));
    }
    if ready.notify {
        return Some("notify".to_string());
    }
    None
}

const PORT_RELEASE_TIMEOUT: Duration = Duration::from_secs(15);
const PORT_RELEASE_INITIAL_DELAY: Duration = Duration::from_millis(25);
const PORT_RELEASE_MAX_DELAY: Duration = Duration::from_millis(250);

fn declared_ports(config: &ProcessConfig) -> Vec<u16> {
    config
        .ports
        .values()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn can_bind_exact_port(port: u16) -> bool {
    bind_no_reuse(socket2::Domain::IPV4, "0.0.0.0", port).is_ok()
}

fn bind_no_reuse(
    domain: socket2::Domain,
    addr: &str,
    port: u16,
) -> Result<TcpListener, std::io::Error> {
    use std::net::SocketAddr;

    let socket = socket2::Socket::new(domain, socket2::Type::STREAM, Some(socket2::Protocol::TCP))?;
    let sock_addr: SocketAddr = format!("{}:{}", addr, port)
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    socket.bind(&socket2::SockAddr::from(sock_addr))?;
    socket.listen(1)?;
    Ok(TcpListener::from(socket))
}

async fn wait_for_port_release(ports: &[u16], timeout: Duration) -> Vec<u16> {
    let started = std::time::Instant::now();
    let mut delay = PORT_RELEASE_INITIAL_DELAY;

    loop {
        let busy: Vec<u16> = ports
            .iter()
            .copied()
            .filter(|port| !can_bind_exact_port(*port))
            .collect();

        if busy.is_empty() || started.elapsed() >= timeout {
            return busy;
        }

        tokio::time::sleep(delay).await;
        delay = Duration::from_secs_f64(
            (delay.as_secs_f64() * 2.0).min(PORT_RELEASE_MAX_DELAY.as_secs_f64()),
        );
    }
}

impl NativeProcessManager {
    /// Create a new native process manager
    pub fn new(state_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&state_dir).into_diagnostic()?;

        Ok(Self {
            processes: Arc::new(RwLock::new(HashMap::new())),
            state_dir,
            shutdown: CancellationToken::new(),
            processes_activity: Arc::new(RwLock::new(None)),
            task_notify: None,
        })
    }

    /// Set the notify handle used to wake the task dependency loop
    /// when process lifecycle changes (e.g. a not-started process is started).
    pub fn set_task_notify(&mut self, notify: Arc<Notify>) {
        self.task_notify = Some(notify);
    }

    /// Query the current lifecycle phase of a process entry.
    pub async fn get_phase(&self, name: &str) -> Option<ProcessPhase> {
        let processes = self.processes.read().await;
        match processes.get(name) {
            Some(ProcessEntry::NotStarted { .. }) => Some(ProcessPhase::NotStarted),
            Some(ProcessEntry::Waiting { .. }) => Some(ProcessPhase::Waiting),
            Some(ProcessEntry::Active(handle)) => Some(handle.status_rx.borrow().phase.into()),
            None => None,
        }
    }

    /// Subscribe to status updates for a given active process.
    /// Returns a clone of the watch receiver if the process is active.
    pub async fn subscribe_status(
        &self,
        name: &str,
    ) -> Option<tokio::sync::watch::Receiver<crate::supervisor_state::JobStatus>> {
        let processes = self.processes.read().await;
        match processes.get(name) {
            Some(ProcessEntry::Active(handle)) => Some(handle.status_rx.clone()),
            _ => None,
        }
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

    /// Create a TUI activity for a process without launching it.
    fn create_process_activity(&self, config: &ProcessConfig, parent_id: Option<u64>) -> Activity {
        let mut ports: Vec<String> = config
            .listen
            .iter()
            .filter_map(|spec| {
                spec.address.as_ref().and_then(|addr| {
                    addr.rsplit(':')
                        .next()
                        .map(|port| format!("{}:{}", spec.name, port))
                })
            })
            .collect();
        let listen_names: std::collections::HashSet<&str> =
            config.listen.iter().map(|s| s.name.as_str()).collect();
        for (name, port) in &config.ports {
            if !listen_names.contains(name.as_str()) {
                ports.push(format!("{}:{}", name, port));
            }
        }

        let mut builder = Activity::process(&config.name)
            .command(&config.exec)
            .ports(ports);
        if let Some(probe_desc) = probe_description(config) {
            builder = builder.ready_probe(probe_desc);
        }
        if let Some(pid) = parent_id {
            builder = builder.parent(Some(pid));
        }
        builder.start()
    }

    /// Register a process as waiting for dependencies.
    ///
    /// Creates the TUI activity with Waiting status without launching.
    /// Call `launch_waiting` after dependencies are satisfied.
    pub async fn register_waiting(&self, config: ProcessConfig, parent_id: Option<u64>) {
        let activity = self.create_process_activity(&config, parent_id);
        activity.set_status(ProcessStatus::Waiting);
        let name = config.name.clone();
        self.processes
            .write()
            .await
            .insert(name.clone(), ProcessEntry::Waiting { config, activity });
        info!("Registered waiting process: {}", name);
    }

    /// Cancel a previously registered waiting process.
    ///
    /// Removes the `Waiting` entry and marks the activity as failed so the
    /// TUI no longer shows the process as "Waiting". Used when a process
    /// task's dependencies fail or are cancelled.
    pub async fn cancel_waiting(&self, name: &str) {
        let mut processes = self.processes.write().await;
        if let Some(ProcessEntry::Waiting { activity, .. }) = processes.remove(name) {
            activity.dependency_failed();
            info!("Cancelled waiting process: {}", name);
        }
    }

    /// Launch a previously registered waiting process.
    ///
    /// Removes the `Waiting` entry, transitions the activity to `Running`
    /// status, and launches the process. The TUI elapsed time includes the
    /// waiting period since the activity was created at registration time.
    pub async fn launch_waiting(&self, name: &str) -> Result<Option<Arc<Job>>> {
        let mut processes = self.processes.write().await;
        let (config, activity) = match processes.remove(name) {
            Some(ProcessEntry::Waiting { config, activity }) => (config, activity),
            Some(entry) => {
                processes.insert(name.to_string(), entry);
                bail!("Process {} is not in waiting state", name)
            }
            None => bail!("Process {} not found", name),
        };
        drop(processes);

        let result = self.launch_or_register_not_started(config, activity).await;

        // Wake any API Wait handlers that are blocked on Waiting entries.
        if let Some(notify) = &self.task_notify {
            notify.notify_waiters();
        }

        result
    }

    /// Start a command with the given configuration.
    ///
    /// If `start.enable` is false, the process is registered as not started (visible
    /// in TUI as stopped but not running) and `Ok(None)` is returned.
    pub async fn start_command(
        &self,
        config: &ProcessConfig,
        parent_id: Option<u64>,
    ) -> Result<Option<Arc<Job>>> {
        debug!("Starting command '{}': {}", config.name, config.exec);

        let activity = self.create_process_activity(config, parent_id);

        self.launch_or_register_not_started(config.clone(), activity)
            .await
    }

    /// Launch a process if enabled, or register as not started if auto start is off.
    ///
    /// Returns `Ok(None)` for auto start off processes, `Ok(Some(job))` for launched ones.
    async fn launch_or_register_not_started(
        &self,
        config: ProcessConfig,
        activity: Activity,
    ) -> Result<Option<Arc<Job>>> {
        if !config.start.enable {
            activity.set_status(ProcessStatus::NotStarted);
            info!("Registered auto start off process: {}", config.name);
            self.processes.write().await.insert(
                config.name.clone(),
                ProcessEntry::NotStarted { config, activity },
            );
            return Ok(None);
        }

        self.launch(&config, activity).await.map(Some)
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

        let process_setup = if has_sockets || has_caps {
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
            Some((fds, capabilities))
        } else {
            None
        };

        // Set spawn hook to configure env, cwd, and stdio on the TokioCommand
        // directly instead of baking them into the bash wrapper script. This
        // avoids hitting the kernel ARG_MAX limit with large environments.
        let spawn_env = proc_cmd.env;
        let spawn_cwd = proc_cmd.cwd;
        let spawn_stdout = proc_cmd.stdout_log.clone();
        let spawn_stderr = proc_cmd.stderr_log.clone();

        job.set_spawn_hook(move |command_wrap, _ctx| {
            let cmd = command_wrap.command_mut();
            cmd.envs(&spawn_env);
            if let Some(ref cwd) = spawn_cwd {
                cmd.current_dir(cwd);
            }
            cmd.stdin(std::process::Stdio::null());
            cmd.stdout(
                crate::command::open_log_file(&spawn_stdout)
                    .map(std::process::Stdio::from)
                    .unwrap_or_else(std::process::Stdio::null),
            );
            cmd.stderr(
                crate::command::open_log_file(&spawn_stderr)
                    .map(std::process::Stdio::from)
                    .unwrap_or_else(std::process::Stdio::null),
            );

            if let Some((ref fds, ref capabilities)) = process_setup {
                command_wrap.wrap(ProcessSetupWrapper::new(fds.clone(), capabilities.clone()));
            }
        });

        job.start().await;

        // Spawn file tailers to emit output to activity
        let stderr_log = proc_cmd.stderr_log.clone();
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
            stderr_log,
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
        let handle = {
            let mut processes = self.processes.write().await;

            match processes.remove(name) {
                Some(ProcessEntry::Active(handle)) => handle,
                Some(entry @ (ProcessEntry::NotStarted { .. } | ProcessEntry::Waiting { .. })) => {
                    let state = if matches!(entry, ProcessEntry::NotStarted { .. }) {
                        "auto start off"
                    } else {
                        "waiting for dependencies"
                    };
                    processes.insert(name.to_string(), entry);
                    bail!("Process {} is {}, cannot stop", name, state)
                }
                None => bail!("Process {} not found", name),
            }
        };

        let grace_period = Duration::from_secs(5);
        let stop_started = std::time::Instant::now();
        let ports = declared_ports(&handle.resources.config);

        debug!("Stopping process: {}", name);
        info!(
            "Stopping process {} and waiting up to {:?} for termination",
            name, grace_period
        );

        handle.resources.activity.log(format!(
            "Shutdown requested; sending SIGTERM and waiting up to {:.1}s",
            grace_period.as_secs_f32()
        ));

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
            .stop_with_signal(Signal::Terminate, grace_period)
            .await;

        if !ports.is_empty() {
            let initially_busy: Vec<u16> = ports
                .iter()
                .copied()
                .filter(|port| !can_bind_exact_port(*port))
                .collect();

            if !initially_busy.is_empty() {
                let port_list = initially_busy
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                handle
                    .resources
                    .activity
                    .log(format!("Waiting for ports to be released: {}", port_list));
            }

            let busy_ports = wait_for_port_release(&ports, PORT_RELEASE_TIMEOUT).await;
            if busy_ports.is_empty() {
                if !initially_busy.is_empty() {
                    handle.resources.activity.log(format!(
                        "Ports released after {:.2}s",
                        stop_started.elapsed().as_secs_f32()
                    ));
                }
            } else {
                let port_list = busy_ports
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                let message = format!(
                    "Ports still busy after {:.1}s: {}",
                    PORT_RELEASE_TIMEOUT.as_secs_f32(),
                    port_list
                );
                handle.resources.activity.log(&message);
                warn!("{} for process {}", message, name);
            }
        }

        let stop_elapsed = stop_started.elapsed();
        handle.resources.activity.reset();
        handle.resources.activity.set_status(ProcessStatus::Stopped);
        handle.resources.activity.log(format!(
            "Process terminated after {:.2}s",
            stop_elapsed.as_secs_f32()
        ));

        info!("Process {} stopped", name);
        Ok(())
    }

    /// Signal all supervisors to shut down gracefully.
    ///
    /// This wakes the supervisor loops so they exit before we abort their tasks.
    pub fn shutdown_supervisors(&self) {
        self.shutdown.cancel();
    }

    /// Stop all processes and clear not-started/waiting entries
    pub async fn stop_all(&self) -> Result<()> {
        debug!("stop_all: shutting down supervisors");
        // Signal supervisors first so they exit gracefully
        self.shutdown_supervisors();

        let names = active_names(&*self.processes.read().await);

        if let Some(activity) = self.processes_activity.read().await.as_ref() {
            if names.is_empty() {
                activity.log("Shutdown requested; no active processes to stop.");
            } else {
                activity.log(format!(
                    "Shutdown requested; stopping {} process(es)...",
                    names.len()
                ));
            }
        }

        debug!("stop_all: stopping {} processes: {:?}", names.len(), names);
        for (name, result) in names
            .iter()
            .zip(futures::future::join_all(names.iter().map(|name| self.stop(name))).await)
        {
            if let Err(err) = result {
                warn!("Failed to stop process {}: {}", name, err);
            }
        }

        if !names.is_empty()
            && let Some(activity) = self.processes_activity.read().await.as_ref()
        {
            activity.log("All managed processes have stopped.");
        }

        // Clear not-started and waiting processes (their activities complete on drop)
        self.processes
            .write()
            .await
            .retain(|_, entry| matches!(entry, ProcessEntry::Active(_)));

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
            Some(_) => {
                debug!("restart: process {} is not active, ignoring", name);
                return Ok(());
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

    /// Start a previously not-started process, reusing its existing TUI activity.
    pub async fn start_not_started(&self, name: &str) -> Result<Arc<Job>> {
        let (config, activity) = {
            let mut processes = self.processes.write().await;
            match processes.get(name) {
                Some(ProcessEntry::NotStarted { .. }) => {}
                Some(_) => bail!("Process {} is not in not-started state", name),
                None => bail!("Process {} not found", name),
            }
            // Safe: we just checked the variant above.
            match processes.remove(name).unwrap() {
                ProcessEntry::NotStarted { config, activity } => (config, activity),
                _ => unreachable!(),
            }
        };

        // Reset the activity so it no longer shows as stopped
        activity.reset();

        info!("Starting not-started process: {}", name);
        // Move the activity into launch (not clone) so the original is not
        // dropped — Activity::drop sends Process::Complete which would
        // immediately mark the process as stopped in the TUI.
        let job = self.launch(&config, activity).await?;

        // Notify the task system so it re-checks dependencies.
        // Dependent processes will be launched by the task scheduler once
        // it sees this dependency's phase has changed.
        if let Some(notify) = &self.task_notify {
            notify.notify_waiters();
        }

        Ok(job)
    }

    /// Get list of running processes
    pub async fn list(&self) -> Vec<String> {
        active_names(&*self.processes.read().await)
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
                Some(_) => {
                    return Ok(());
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
        let task_notify = self.task_notify.clone();

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
                        let task_notify = task_notify.clone();
                        tokio::spawn(Self::handle_api_client(stream, processes, task_notify));
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
        task_notify: Option<Arc<Notify>>,
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
                // Poll until no Waiting entries remain and all Active processes
                // are ready. This avoids a race where the API server starts
                // before processes transition from Waiting to Active.
                loop {
                    // Register and enable the notification BEFORE checking state
                    // to prevent missed wakeups (same pattern as wait_for_task_deps).
                    let notified = task_notify.as_ref().map(|n| n.notified());
                    tokio::pin!(notified);
                    if let Some(n) = notified.as_mut().as_pin_mut() {
                        n.enable();
                    }

                    let procs = processes.read().await;
                    let has_waiting = procs
                        .values()
                        .any(|e| matches!(e, ProcessEntry::Waiting { .. }));

                    if has_waiting {
                        drop(procs);
                        match notified.as_pin_mut() {
                            Some(notified) => {
                                tokio::select! {
                                    _ = notified => {},
                                    _ = tokio::time::sleep(Duration::from_secs(1)) => {},
                                }
                            }
                            None => {
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                        }
                        continue;
                    }

                    let receivers: Vec<(
                        String,
                        tokio::sync::watch::Receiver<crate::supervisor_state::JobStatus>,
                    )> = procs
                        .iter()
                        .filter_map(|(name, entry)| match entry {
                            ProcessEntry::Active(handle) => {
                                Some((name.clone(), handle.status_rx.clone()))
                            }
                            ProcessEntry::NotStarted { .. } | ProcessEntry::Waiting { .. } => None,
                        })
                        .collect();
                    drop(procs);

                    for (name, mut rx) in receivers {
                        {
                            let status = rx.borrow_and_update();
                            if status.is_ready() || status.is_gave_up() {
                                continue;
                            }
                        }
                        debug!("API: waiting for process {} to become ready", name);
                        while rx.changed().await.is_ok() {
                            let status = rx.borrow();
                            if status.is_ready() || status.is_gave_up() {
                                break;
                            }
                        }
                    }

                    break;
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

    /// Handle a single process command (restart, start not-started, etc.).
    pub async fn handle_command(&self, cmd: ProcessCommand) {
        match cmd {
            ProcessCommand::Restart(name) => {
                let is_not_started = matches!(
                    self.processes.read().await.get(&name),
                    Some(ProcessEntry::NotStarted { .. })
                );
                if is_not_started {
                    info!("Starting not-started process: {}", name);
                    if let Err(e) = self.start_not_started(&name).await {
                        warn!("Failed to start process {}: {}", name, e);
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

    /// Start processing commands in a background task.
    ///
    /// This allows commands (e.g. Ctrl-R restart) to be handled while task
    /// execution is still in progress. The background task exits when the
    /// sender half of the channel is dropped.
    pub fn start_command_listener(self: &Arc<Self>, mut rx: mpsc::Receiver<ProcessCommand>) {
        let pm = Arc::clone(self);
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                pm.handle_command(cmd).await;
            }
        });
    }

    /// Note: This method relies on the cancellation token for shutdown signals.
    /// Signal handling (SIGINT/SIGTERM) is done by tokio-shutdown in the main app,
    /// which cancels the token when a signal is received.
    pub async fn run_foreground(
        &self,
        cancellation_token: tokio_util::sync::CancellationToken,
        mut command_rx: Option<mpsc::Receiver<ProcessCommand>>,
    ) -> Result<()> {
        debug!(
            "run_foreground: ENTERED, token_cancelled={}",
            cancellation_token.is_cancelled()
        );
        info!("Manager event loop started (foreground)");

        loop {
            tokio::select! {
                biased;
                _ = cancellation_token.cancelled() => {
                    debug!("run_foreground: cancellation detected, calling stop_all");
                    info!("Shutdown requested, stopping all processes");
                    self.stop_all().await?;
                    debug!("run_foreground: stop_all completed");
                    break;
                }
                Some(cmd) = async {
                    match command_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    self.handle_command(cmd).await;
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

    /// Start processes from the given configs, filtering by name if specified.
    ///
    /// Processes with dependencies (`after`) are registered in the TUI as "waiting"
    /// and launched concurrently once their dependencies are satisfied.
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
    use crate::config::{RestartPolicy, StartConfig};
    use std::net::Ipv4Addr;

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

    fn test_config(name: &str) -> ProcessConfig {
        ProcessConfig {
            name: name.to_string(),
            exec: "echo".to_string(),
            args: vec!["hello".to_string()],
            restart: crate::config::RestartConfig {
                on: RestartPolicy::Never,
                max: Some(5),
                window: None,
            },
            ..Default::default()
        }
    }

    fn auto_start_off_config(name: &str) -> ProcessConfig {
        ProcessConfig {
            start: StartConfig { enable: false },
            ..test_config(name)
        }
    }

    fn long_running_config(name: &str) -> ProcessConfig {
        ProcessConfig {
            name: name.to_string(),
            exec: "sleep".to_string(),
            args: vec!["100".to_string()],
            restart: crate::config::RestartConfig {
                on: RestartPolicy::Never,
                max: Some(5),
                window: None,
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_register_waiting_sets_phase() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
        let config = test_config("waiter");

        manager.register_waiting(config, None).await;

        assert_eq!(
            manager.get_phase("waiter").await,
            Some(ProcessPhase::Waiting)
        );

        let _ = manager.stop_all().await;
    }

    #[tokio::test]
    async fn test_get_phase_unknown() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();

        assert_eq!(manager.get_phase("nonexistent").await, None);
    }

    #[tokio::test]
    async fn test_cancel_waiting_removes_entry() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
        let config = test_config("cancel-me");

        manager.register_waiting(config, None).await;
        assert_eq!(
            manager.get_phase("cancel-me").await,
            Some(ProcessPhase::Waiting)
        );

        manager.cancel_waiting("cancel-me").await;
        assert_eq!(manager.get_phase("cancel-me").await, None);
    }

    #[tokio::test]
    async fn test_cancel_waiting_noop_for_unknown() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();

        // Should not panic
        manager.cancel_waiting("does-not-exist").await;
    }

    #[tokio::test]
    async fn test_launch_waiting_auto_start_off_becomes_not_started() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
        let config = auto_start_off_config("auto-start-off-proc");

        manager.register_waiting(config, None).await;
        let result = manager.launch_waiting("auto-start-off-proc").await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
        assert_eq!(
            manager.get_phase("auto-start-off-proc").await,
            Some(ProcessPhase::NotStarted)
        );
    }

    #[tokio::test]
    async fn test_launch_waiting_not_found_errors() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();

        let result = manager.launch_waiting("ghost").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_launch_waiting_not_in_waiting_state_errors() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
        let config = test_config("active-proc");

        manager.start_command(&config, None).await.unwrap();

        let result = manager.launch_waiting("active-proc").await;
        assert!(result.is_err());

        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("not in waiting state"),
            "Expected error about not being in waiting state, got: {}",
            err_msg
        );

        let _ = manager.stop_all().await;
    }

    #[tokio::test]
    async fn test_launch_waiting_enabled_starts_process() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
        let config = long_running_config("long-runner");

        manager.register_waiting(config, None).await;
        let result = manager.launch_waiting("long-runner").await;

        assert!(result.is_ok());
        let job = result.unwrap();
        assert!(job.is_some(), "Expected Some(job) for an enabled process");

        let phase = manager.get_phase("long-runner").await;
        assert_ne!(phase, Some(ProcessPhase::Waiting));

        let _ = manager.stop_all().await;
    }

    #[tokio::test]
    async fn test_launch_waiting_notifies_task_system() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();

        let notify = Arc::new(Notify::new());
        manager.set_task_notify(notify.clone());

        let config = auto_start_off_config("notify-proc");
        manager.register_waiting(config, None).await;

        // Register the notified future before launch_waiting so the
        // notification is not missed due to a race.
        let notified = notify.notified();
        tokio::pin!(notified);

        let _ = manager.launch_waiting("notify-proc").await;

        let completed = tokio::time::timeout(std::time::Duration::from_secs(5), notified).await;

        assert!(
            completed.is_ok(),
            "Notification should have fired within the timeout"
        );
    }

    #[tokio::test]
    async fn test_wait_for_port_release_waits_until_port_is_bindable() {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        assert!(
            !can_bind_exact_port(port),
            "test listener should hold the port before release"
        );

        let started = std::time::Instant::now();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            drop(listener);
        });

        let busy_ports = wait_for_port_release(&[port], Duration::from_secs(1)).await;

        assert!(busy_ports.is_empty(), "port should have been released");
        assert!(
            started.elapsed() >= Duration::from_millis(150),
            "expected to wait for the listener to close"
        );
    }

    #[tokio::test]
    async fn test_wait_for_port_release_returns_busy_ports_on_timeout() {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        let busy_ports = wait_for_port_release(&[port], Duration::from_millis(50)).await;

        assert_eq!(busy_ports, vec![port]);
        drop(listener);
    }
}
