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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};

/// Request sent by a client to the native manager API socket.
///
/// Protocol: newline-delimited JSON over a Unix stream socket.
/// The client sends one `ApiRequest` per line, the server responds with one `ApiResponse`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum ApiRequest {
    /// Block until every managed process is ready, then respond.
    Wait,
    /// List all managed processes and their status.
    List,
    /// Get the status of a single process.
    Status { name: String },
    /// Get the last N lines of stdout/stderr logs for a process.
    Logs { name: String, lines: Option<usize> },
    /// Restart a running process in place (or bring a stopped one back
    /// through the scheduler, honouring its dependencies).
    Restart { name: String },
    /// Start the named processes, honouring their `after`/`before`
    /// dependencies. Driven by the task scheduler that owns this manager, so
    /// already-running and out-of-subset dependencies resolve against the
    /// live task graph; explicitly named processes start even with
    /// `start.enable = false`. Used by `devenv up` attaching to a running
    /// manager (the client resolves the up-enabled default set before
    /// sending) and by `devenv processes start`.
    Start { names: Vec<String> },
    /// Stop a running process.
    Stop { name: String },
    /// Query all port allocations from running processes.
    Ports,
    /// Hold the connection open and stream `AttachEvent` lines (snapshot,
    /// status changes, log lines) until the client disconnects or the
    /// manager shuts down.
    Attach,
    /// Ask whether the running manager resides in this process or a daemon.
    /// Answered authoritatively by the live manager itself.
    #[serde(rename = "mode")]
    Residence,
}

/// Where the running native manager resides. The manager answers this over its
/// control socket ([`ApiRequest::Residence`]), so the live process is the
/// single source of truth for its residence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerResidence {
    /// A live devenv process owns the manager, whether following it
    /// interactively or retaining it for a command such as `devenv test`. A
    /// `devenv up -d` from another terminal must not schedule into it.
    #[serde(rename = "foreground")]
    InProcess,
    /// A detached daemon spawned by `devenv up -d` owns the manager; a later
    /// `devenv up` attaches and schedules into it.
    Daemon,
}

/// Port allocation info from a running process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortInfo {
    pub process_name: String,
    pub port_name: String,
    pub port: u16,
}

/// Summary information about a managed process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub name: String,
    pub phase: ProcessPhase,
    pub restart_count: u64,
    /// Configured ports, formatted as "name:port" (e.g. ["http:8080"]).
    #[serde(default)]
    pub ports: Vec<String>,
}

/// Response sent by the native manager API socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ApiResponse {
    /// All processes are ready.
    Ready,
    /// An error occurred.
    Error { message: String },
    /// List of all managed processes.
    ProcessList { processes: Vec<ProcessInfo> },
    /// Detailed info about a single process.
    ProcessDetail { info: ProcessInfo },
    /// Log output for a process.
    ProcessLogs { stdout: String, stderr: String },
    /// Operation completed successfully.
    Ok,
    /// All port allocations from managed processes.
    PortAllocations { ports: Vec<PortInfo> },
    /// Result of a `Start` request: how each requested name was classified.
    Start { outcome: StartOutcome },
    /// Where the running manager resides.
    #[serde(rename = "mode")]
    Residence {
        #[serde(rename = "mode")]
        residence: ManagerResidence,
    },
}

/// Outcome of starting a set of processes via the owning scheduler.
/// Each requested name lands in exactly one bucket.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StartOutcome {
    /// Newly armed: re-registered Waiting and handed to the dependency-driven
    /// launch path.
    #[serde(default)]
    pub scheduled: Vec<String>,
    /// Already running, starting, or pending on a dependency: left untouched.
    #[serde(default)]
    pub skipped: Vec<String>,
    /// Not present in the manager's task graph (the manager was started with a
    /// different configuration or a subset of processes).
    #[serde(default)]
    pub unknown: Vec<String>,
    /// Known but could not be scheduled (e.g. building the process config
    /// failed).
    #[serde(default)]
    pub failed: Vec<String>,
}

/// Result of asking a process to restart. The process owner decides whether
/// its retained controller can perform the restart; callers never infer this
/// from presentation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum RestartOutcome {
    RestartedInPlace,
    SchedulingRequired,
}

/// Event pushed by the daemon on an `ApiRequest::Attach` connection.
/// Newline-delimited JSON, one event per line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AttachEvent {
    /// Full process list, sent once when the stream opens.
    #[serde(rename = "snapshot")]
    InitialState { processes: Vec<ProcessInfo> },
    /// A process changed phase/ports/restart count, or newly appeared.
    Status { info: ProcessInfo },
    /// One log line from a process log file (backlog or live tail).
    Log {
        name: String,
        stream: LogStream,
        line: String,
    },
}

/// Which output stream an `AttachEvent::Log` line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    Stdout,
    Stderr,
}

/// Live attach stream to a running manager. Dropping it closes the connection.
pub struct AttachStream {
    rx: mpsc::Receiver<Result<AttachEvent>>,
    reader_task: JoinHandle<()>,
}

impl AttachStream {
    /// Next event from the daemon; `None` means the daemon closed the stream.
    pub async fn next(&mut self) -> Option<Result<AttachEvent>> {
        self.rx.recv().await
    }
}

impl Drop for AttachStream {
    fn drop(&mut self) {
        self.reader_task.abort();
    }
}

use watchexec_supervisor::job::{Job, start_job};

use crate::ProcessManagerControl;
use crate::config::{ProcessConfig, ShutdownConfig};
use crate::pid::{self, PidStatus};
use crate::socket_activation::{ProcessSetupWrapper, activation_from_listen};
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
    pub activity: devenv_activity::ActivityRef,
    pub notify_socket: Option<Arc<NotifySocket>>,
    pub status_tx: tokio::sync::watch::Sender<crate::supervisor_state::JobStatus>,
    pub stderr_log: PathBuf,
    /// Process scopes created by this process across restarts.
    pub(crate) scopes: Arc<crate::process_guardian::ProcessScopeRegistry>,
    /// Shared count of started processes that have not reached a terminal phase.
    pub(crate) live: Arc<AtomicUsize>,
    /// Notified after a process reaches a terminal phase.
    pub(crate) completion: Arc<Notify>,
    /// Per-process stop intent, visible while the supervisor awaits cleanup.
    pub(crate) stop_requested: CancellationToken,
}

/// Handle to a managed process job
pub struct JobHandle {
    pub resources: ProcessResources,
    /// Status receiver for querying supervisor state
    pub status_rx: tokio::sync::watch::Receiver<crate::supervisor_state::JobStatus>,
    /// Supervisor task handling restarts
    pub supervisor_task: JoinHandle<()>,
    /// Channel to send lifecycle commands (restart/stop) into the supervisor loop.
    pub cmd_tx: mpsc::Sender<crate::supervisor::SupervisorCommand>,
    /// Output reader tasks (stdout, stderr)
    pub output_readers: Option<(JoinHandle<()>, JoinHandle<()>)>,
    /// Forwards supervisor status transitions to the task system; exits when
    /// the status channel closes. Aborted together with the supervisor.
    pub notify_forwarder: JoinHandle<()>,
}

/// Lifecycle phase of a managed process.
///
/// Shared between the process manager and the task system to avoid
/// duplicate enum definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessPhase {
    /// Process has `start.enable = false`; not yet launched.
    NotStarted,
    /// Process was explicitly stopped by the user.
    Stopped,
    /// Registered, waiting for dependencies before starting.
    Waiting,
    /// Launched, readiness not yet confirmed.
    Starting,
    /// Readiness probe passed.
    Ready,
    /// Teardown in progress.
    Stopping,
    /// Process exited and will not be restarted.
    Exited,
    /// Supervisor gave up (crash loop).
    GaveUp,
}

impl std::fmt::Display for ProcessPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotStarted => write!(f, "not_started"),
            Self::Stopped => write!(f, "stopped"),
            Self::Waiting => write!(f, "waiting"),
            Self::Starting => write!(f, "starting"),
            Self::Ready => write!(f, "ready"),
            Self::Stopping => write!(f, "stopping"),
            Self::Exited => write!(f, "exited"),
            Self::GaveUp => write!(f, "gave_up"),
        }
    }
}

/// Controls manager and runner event loops after all processes settle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnIdle {
    /// Stay alive until shutdown.
    Linger,
    /// Return once every started process reaches a terminal phase.
    Exit,
}

fn initial_status(config: &ProcessConfig) -> crate::process_state::ProcessStatus {
    crate::process_state::ProcessStatus::running(
        config.has_readiness_probe(),
        crate::process_state::StateTransition::Launching,
    )
}

/// Prevent a registered process from exposing logs from an earlier manager.
fn clear_stale_logs(state_dir: &Path, name: &str) {
    let (stdout_path, stderr_path) = crate::command::log_paths(state_dir, name);
    let _ = std::fs::write(&stdout_path, "");
    let _ = std::fs::write(&stderr_path, "");
}

/// Resources consumed by [`ProcessRunner::finish_stop`].
struct StopParts {
    job: Arc<Job>,
    cmd_tx: mpsc::Sender<crate::supervisor::SupervisorCommand>,
    supervisor_task: JoinHandle<()>,
    notify_forwarder: JoinHandle<()>,
    output_readers: Option<(JoinHandle<()>, JoinHandle<()>)>,
    ports: Vec<u16>,
    shutdown: ShutdownConfig,
    scopes: Arc<crate::process_guardian::ProcessScopeRegistry>,
    reason: crate::StopReason,
}

/// Replace an active entry with an atomic teardown placeholder.
/// `user_stopped` affects display only; terminal history is always retained.
fn take_active_for_stop(
    entry: &mut ProcessEntry,
    handle: JobHandle,
    user_stopped: bool,
) -> StopParts {
    handle.resources.stop_requested.cancel();
    let ports = declared_ports(&handle.resources.config);
    let current = *entry.status_rx.borrow();
    let mut stopping = current;
    let reason = if user_stopped {
        crate::process_state::StopReason::User
    } else {
        crate::process_state::StopReason::ManagerShutdown
    };
    stopping.target = crate::process_state::TargetState::Stopped(reason);
    if stopping.restart == crate::process_state::RestartDecision::Pending {
        stopping.restart = crate::process_state::RestartDecision::None;
    }
    if stopping.child == crate::process_state::ChildState::Running {
        stopping.transition = Some(crate::process_state::StateTransition::Terminating);
    }
    entry.publish(stopping);
    let JobHandle {
        resources,
        cmd_tx,
        supervisor_task,
        notify_forwarder,
        output_readers,
        ..
    } = handle;
    let job = resources.job;
    let scopes = resources.scopes;
    let shutdown = resources.config.shutdown;

    StopParts {
        job,
        cmd_tx,
        supervisor_task,
        notify_forwarder,
        output_readers,
        ports,
        shutdown,
        scopes,
        reason,
    }
}

/// Ask a supervisor to stop its job and wait for its task to end.
async fn stop_via_supervisor(
    cmd_tx: &mpsc::Sender<crate::supervisor::SupervisorCommand>,
    supervisor_task: JoinHandle<()>,
) {
    let (ack_tx, ack_rx) = oneshot::channel();
    if cmd_tx
        .send(crate::supervisor::SupervisorCommand::Stop { ack: ack_tx })
        .await
        .is_ok()
    {
        let _ = ack_rx.await;
    }
    let _ = supervisor_task.await;
}

/// Collect the names of all active (supervised) processes from the map.
fn active_names(processes: &HashMap<String, ProcessEntry>) -> Vec<String> {
    processes
        .iter()
        .filter_map(|(name, entry)| entry.handle.is_some().then(|| name.clone()))
        .collect()
}

/// Stable registry record. Lifecycle facts and controller ownership are
/// orthogonal; neither is encoded in the container shape.
struct ProcessEntry {
    config: ProcessConfig,
    activity: Activity,
    status_tx: tokio::sync::watch::Sender<crate::process_state::ProcessStatus>,
    status_rx: tokio::sync::watch::Receiver<crate::process_state::ProcessStatus>,
    handle: Option<JobHandle>,
    /// The one stable notify socket for this process. It survives stop/start
    /// cycles and is drained between child runs.
    notify_socket: Option<Arc<NotifySocket>>,
}

impl ProcessEntry {
    fn new(config: ProcessConfig, activity: Activity, status: crate::ProcessStatus) -> Self {
        activity.set_status(status.activity_status());
        let (status_tx, status_rx) = tokio::sync::watch::channel(status);
        Self {
            config,
            activity,
            status_tx,
            status_rx,
            handle: None,
            notify_socket: None,
        }
    }

    fn config(&self) -> &ProcessConfig {
        &self.config
    }

    fn status(&self) -> crate::ProcessStatus {
        *self.status_rx.borrow()
    }

    fn publish(&self, next: crate::ProcessStatus) {
        if !next.is_valid() {
            tracing::error!(
                process = %self.config.name,
                current = ?self.status(),
                rejected = ?next,
                "rejecting invalid process status publication"
            );
            return;
        }
        self.status_tx.send_if_modified(|current| {
            if *current == next {
                false
            } else {
                *current = next;
                true
            }
        });
        self.activity.set_status(next.activity_status());
    }

    fn reset_for_waiting(&mut self, config: ProcessConfig) {
        self.config = config;
        self.activity.reset();
        self.publish(crate::ProcessStatus::waiting());
    }

    fn can_prepare_start(&self) -> bool {
        self.handle.is_none()
            && self.status().transition.is_none()
            && self.status().child != crate::ChildState::Running
    }

    fn is_launching(&self) -> bool {
        let status = self.status();
        self.handle.is_none()
            && status.transition == Some(crate::StateTransition::Launching)
            && status.target == crate::TargetState::Running
    }

    fn start_launch(&self) {
        let mut status = self.status();
        status.target = crate::TargetState::Running;
        status.transition = Some(crate::StateTransition::Launching);
        status.child = crate::ChildState::NeverSpawned;
        status.readiness = crate::ReadinessState::Inactive;
        status.restart = crate::RestartDecision::None;
        self.publish(status);
    }

    fn finish_launch_failure(&self) {
        let mut status = self.status();
        status.target = crate::TargetState::Stopped(crate::StopReason::LaunchFailure);
        status.transition = None;
        status.child = crate::ChildState::NeverSpawned;
        status.readiness = crate::ReadinessState::Inactive;
        status.restart = crate::RestartDecision::None;
        self.publish(status);
    }

    fn finish_controlled_stop(&self, reason: crate::StopReason) {
        let mut status = self.status();
        // The supervisor publishes child observations while teardown is in
        // progress. Reassert the stop request's higher-precedence target when
        // the actor-owned teardown settles so a late child update cannot
        // restore TargetState::Running.
        status.target = crate::TargetState::Stopped(reason);
        if status.child == crate::ChildState::Running {
            status.child = crate::ChildState::Terminated;
            status.readiness = crate::ReadinessState::Inactive;
        }
        status.transition = None;
        self.publish(status);
    }
}

/// Runs and supervises native child processes.
///
/// This type deliberately has no API, PID-file, or socket ownership. It is a
/// reusable execution component for both persistent managers and transient
/// task runs.
pub struct ProcessRunner {
    processes: Arc<RwLock<HashMap<String, ProcessEntry>>>,
    state_dir: PathBuf,
    shutdown: CancellationToken,
    /// Parent activity for grouping all processes under "Starting processes"
    /// Optional notify handle fired when a process lifecycle changes (e.g. not-started
    /// process is manually started). The task system uses this to re-check dependencies.
    task_notify: Option<Arc<Notify>>,
    /// Fired on process-map transitions.
    entries_changed: Arc<Notify>,
    /// Number of started processes that have not reached a terminal phase.
    live: Arc<AtomicUsize>,
    /// Wakes a foreground run when a process reaches a terminal phase.
    completion: Arc<Notify>,
}

/// Path-only control client for an already-running native manager.
pub struct NativeManagerClient {
    state_dir: PathBuf,
}

impl NativeManagerClient {
    pub fn new(state_dir: PathBuf) -> Self {
        Self { state_dir }
    }

    fn manager_pid_file(&self) -> PathBuf {
        self.state_dir.join("native-manager.pid")
    }
}

/// Display ports for a process: socket-activation `listen` specs plus declared
/// `ports` not shadowed by a same-named listen spec; "name:port", deduped by
/// name, sorted.
pub fn display_ports(config: &ProcessConfig) -> Vec<String> {
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
    ports.sort();
    ports
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

#[cfg(test)]
fn can_bind_exact_port(port: u16) -> bool {
    bind_no_reuse(socket2::Domain::IPV4, "0.0.0.0", port).is_ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PortReleaseState {
    Free,
    Ownerless,
    Owned(String),
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PortOwnerLookup {
    Ownerless,
    Owned(String),
    Unknown(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PortReleaseStatus {
    ownerless: Vec<u16>,
    owned: Vec<(u16, String)>,
    unknown: Vec<(u16, String)>,
}

impl PortReleaseStatus {
    fn blocking_ports(&self) -> Vec<u16> {
        self.owned
            .iter()
            .map(|(port, _)| *port)
            .chain(self.unknown.iter().map(|(port, _)| *port))
            .collect()
    }

    fn ownerless_ports(&self) -> &[u16] {
        &self.ownerless
    }

    fn has_only_ownerless_conflicts(&self) -> bool {
        !self.ownerless.is_empty() && self.owned.is_empty() && self.unknown.is_empty()
    }
}

fn lookup_process_using_port(port: u16) -> PortOwnerLookup {
    use netstat2::{AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, get_sockets_info};

    let af_flags = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
    let proto_flags = ProtocolFlags::TCP;

    let sockets = match get_sockets_info(af_flags, proto_flags) {
        Ok(sockets) => sockets,
        Err(err) => {
            return PortOwnerLookup::Unknown(format!("socket inspection failed: {}", err));
        }
    };

    for socket in sockets {
        let local_port = match &socket.protocol_socket_info {
            ProtocolSocketInfo::Tcp(tcp) => tcp.local_port,
            ProtocolSocketInfo::Udp(udp) => udp.local_port,
        };

        if local_port == port
            && let Some(&pid) = socket.associated_pids.first()
        {
            #[cfg(target_os = "linux")]
            if let Ok(name) = std::fs::read_to_string(format!("/proc/{}/comm", pid)) {
                return PortOwnerLookup::Owned(format!(" by {} (PID {})", name.trim(), pid));
            }

            return PortOwnerLookup::Owned(format!(" (PID {})", pid));
        }
    }

    PortOwnerLookup::Ownerless
}

/// Bind a TCP socket without `SO_REUSEADDR` to reliably detect port conflicts.
///
/// Mirrors the implementation in `devenv-core::ports` but kept local to avoid
/// adding a cross-crate dependency for a single helper.
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

fn probe_port_release(port: u16) -> PortReleaseState {
    match bind_no_reuse(socket2::Domain::IPV4, "0.0.0.0", port) {
        Ok(listener) => {
            drop(listener);
            PortReleaseState::Free
        }
        Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
            match lookup_process_using_port(port) {
                PortOwnerLookup::Ownerless => PortReleaseState::Ownerless,
                PortOwnerLookup::Owned(owner) => PortReleaseState::Owned(owner),
                PortOwnerLookup::Unknown(reason) => PortReleaseState::Unknown(reason),
            }
        }
        Err(err) => PortReleaseState::Unknown(err.to_string()),
    }
}

fn probe_port_release_status_with<Probe>(ports: &[u16], mut probe: Probe) -> PortReleaseStatus
where
    Probe: FnMut(u16) -> PortReleaseState,
{
    let mut status = PortReleaseStatus::default();

    for port in ports.iter().copied() {
        match probe(port) {
            PortReleaseState::Free => {}
            PortReleaseState::Ownerless => status.ownerless.push(port),
            PortReleaseState::Owned(owner) => status.owned.push((port, owner)),
            PortReleaseState::Unknown(reason) => status.unknown.push((port, reason)),
        }
    }

    status
}

async fn wait_for_port_conflicts_to_settle_with<Probe, Sleep, Fut>(
    ports: &[u16],
    timeout: Duration,
    mut probe: Probe,
    mut sleep: Sleep,
) -> PortReleaseStatus
where
    Probe: FnMut(u16) -> PortReleaseState,
    Sleep: FnMut(Duration) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let started = std::time::Instant::now();
    let mut delay = PORT_RELEASE_INITIAL_DELAY;

    loop {
        let status = probe_port_release_status_with(ports, &mut probe);

        if status.blocking_ports().is_empty()
            || status.has_only_ownerless_conflicts()
            || started.elapsed() >= timeout
        {
            return status;
        }

        sleep(delay).await;
        delay = Duration::from_secs_f64(
            (delay.as_secs_f64() * 2.0).min(PORT_RELEASE_MAX_DELAY.as_secs_f64()),
        );
    }
}

async fn wait_for_port_conflicts_to_settle(ports: &[u16], timeout: Duration) -> PortReleaseStatus {
    wait_for_port_conflicts_to_settle_with(ports, timeout, probe_port_release, tokio::time::sleep)
        .await
}

/// Everything a launch produces before the entry settles to Active.
struct LaunchSetup {
    job: Arc<Job>,
    notify_socket: Option<Arc<NotifySocket>>,
    stdout_tailer: JoinHandle<()>,
    stderr_tailer: JoinHandle<()>,
    stderr_log: PathBuf,
    shutdown: ShutdownConfig,
    scopes: Arc<crate::process_guardian::ProcessScopeRegistry>,
}

impl LaunchSetup {
    /// Tear down a launch that never settled to `Active`: abort the output
    /// tailers and stop the spawned child with the configured grace period.
    /// Used when shutdown raced the launch or the entry changed underneath it.
    async fn abort_and_stop(self) {
        self.stdout_tailer.abort();
        self.stderr_tailer.abort();
        crate::process_guardian::stop_job(&self.job, &self.scopes, &self.shutdown).await;
    }
}

/// Wake everyone observing the process map: the owning task scheduler's
/// dependency loop and internal waiters.
fn notify_lifecycle_parts(entries_changed: &Notify, task_notify: &Option<Arc<Notify>>) {
    entries_changed.notify_waiters();
    if let Some(notify) = task_notify {
        notify.notify_waiters();
    }
}

/// Forward supervisor status transitions to the task system; exits when the
/// status channel closes. Aborted together with the supervisor.
fn spawn_notify_forwarder(
    task_notify: Option<Arc<Notify>>,
    entries_changed: Arc<Notify>,
    mut status_rx: tokio::sync::watch::Receiver<crate::supervisor_state::JobStatus>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if status_rx.changed().await.is_err() {
                break;
            }
            notify_lifecycle_parts(&entries_changed, &task_notify);
        }
        // channel closed: entry removed or supervisor torn down
        notify_lifecycle_parts(&entries_changed, &task_notify);
    })
}

impl ProcessRunner {
    /// Create a native process runner.
    pub fn new(state_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&state_dir).into_diagnostic()?;

        Ok(Self {
            processes: Arc::new(RwLock::new(HashMap::new())),
            state_dir,
            shutdown: CancellationToken::new(),
            task_notify: None,
            entries_changed: Arc::new(Notify::new()),
            live: Arc::new(AtomicUsize::new(0)),
            completion: Arc::new(Notify::new()),
        })
    }

    /// Set the notify handle used to wake the task dependency loop
    /// when process lifecycle changes (e.g. a not-started process is started).
    pub fn set_task_notify(&mut self, notify: Arc<Notify>) {
        self.task_notify = Some(notify);
    }

    /// Wake everyone observing the process map: the owning task scheduler's
    /// dependency loop and internal waiters.
    fn notify_lifecycle(&self) {
        notify_lifecycle_parts(&self.entries_changed, &self.task_notify);
    }

    /// The compatibility phase to display for a process.
    pub async fn get_phase(&self, name: &str) -> Option<ProcessPhase> {
        self.processes
            .read()
            .await
            .get(name)
            .map(|entry| entry.status().display_phase())
    }

    /// Exit result for a process that reached a terminal child exit.
    pub async fn get_exit_outcome(
        &self,
        name: &str,
    ) -> Option<crate::supervisor_state::ExitOutcome> {
        let processes = self.processes.read().await;
        processes
            .get(name)
            .and_then(|entry| entry.status().child.exit_outcome())
    }

    /// Subscribe to status updates for a given active process.
    /// Returns a clone of the watch receiver if the process is active.
    pub async fn subscribe_status(
        &self,
        name: &str,
    ) -> Option<tokio::sync::watch::Receiver<crate::supervisor_state::JobStatus>> {
        let processes = self.processes.read().await;
        processes
            .get(name)
            .filter(|entry| entry.handle.is_some())
            .map(|entry| entry.status_rx.clone())
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
        self.state_dir.join(crate::NATIVE_SOCKET_NAME)
    }

    /// Cancellation token shared by the process supervisors and API streams.
    pub fn shutdown_token(&self) -> &CancellationToken {
        &self.shutdown
    }

    /// Lifecycle notifier used by persistent views to observe process-map changes.
    pub fn entries_changed(&self) -> &Notify {
        &self.entries_changed
    }

    /// Return a sorted snapshot of all managed processes.
    pub async fn process_infos(&self) -> Vec<ProcessInfo> {
        let processes = self.processes.read().await;
        let mut infos: Vec<_> = processes
            .iter()
            .map(|(name, entry)| Self::process_info(name, entry))
            .collect();
        infos.sort_by(|left, right| left.name.cmp(&right.name));
        infos
    }

    /// Return information about one managed process.
    pub async fn process_info_by_name(&self, name: &str) -> Option<ProcessInfo> {
        let processes = self.processes.read().await;
        processes
            .get(name)
            .map(|entry| Self::process_info(name, entry))
    }

    /// Return all declared port allocations in stable order.
    pub async fn port_allocations(&self) -> Vec<PortInfo> {
        let processes = self.processes.read().await;
        let mut ports = Vec::new();
        for (process_name, entry) in processes.iter() {
            for (port_name, &port) in &entry.config().ports {
                ports.push(PortInfo {
                    process_name: process_name.clone(),
                    port_name: port_name.clone(),
                    port,
                });
            }
        }
        ports.sort_by(|left, right| {
            (&left.process_name, &left.port_name, left.port).cmp(&(
                &right.process_name,
                &right.port_name,
                right.port,
            ))
        });
        ports
    }

    fn process_info(name: &str, entry: &ProcessEntry) -> ProcessInfo {
        let status = entry.status();
        ProcessInfo {
            name: name.to_string(),
            phase: status.display_phase(),
            restart_count: status.restart_count,
            ports: display_ports(entry.config()),
        }
    }

    /// Create a TUI activity for a process without launching it.
    fn create_process_activity(&self, config: &ProcessConfig, parent_id: Option<u64>) -> Activity {
        let ports = display_ports(config);

        let mut builder = Activity::process(&config.name)
            .command(&config.exec)
            .ports(ports);
        if let Some(probe_desc) = probe_description(config) {
            builder = builder.ready_probe(probe_desc);
        }
        if let Some(pid) = parent_id {
            builder = builder.parent(Some(pid));
        }
        devenv_activity::start!(builder)
    }

    /// Register a process as waiting for dependencies.
    ///
    /// Creates the TUI activity with Waiting status without launching.
    /// Call `launch_waiting` after dependencies are satisfied.
    pub async fn register_waiting(&self, config: ProcessConfig, parent_id: Option<u64>) {
        let activity = self.create_process_activity(&config, parent_id);
        let name = config.name.clone();
        clear_stale_logs(&self.state_dir, &name);
        self.processes.write().await.insert(
            name.clone(),
            ProcessEntry::new(config, activity, crate::ProcessStatus::waiting()),
        );
        info!("Registered waiting process: {}", name);
        self.notify_lifecycle();
    }

    /// Re-arm a process as `Waiting` so it can be (re)launched by the task
    /// scheduler, unless it is already active.
    ///
    /// Used by `Tasks::start_with_deps` when a later `devenv up` brings up more
    /// processes against an already-running manager: a process that was
    /// registered auto-start-off (`NotStarted`) or was previously `Stopped`
    /// must go back to `Waiting` with the caller's (force-enabled) config so the
    /// normal dependency-driven launch path applies. Already-active processes
    /// are left untouched.
    pub async fn rearm_waiting(&self, config: ProcessConfig) {
        let mut processes = self.processes.write().await;
        // Checked under the write lock so a re-arm racing shutdown can never
        // insert a Waiting entry after stop_all's drain has completed.
        if self.shutdown.is_cancelled() {
            return;
        }
        let name = config.name.clone();
        if let Some(entry) = processes.get_mut(&name) {
            if !entry.can_prepare_start() {
                return;
            }
            entry.reset_for_waiting(config);
        } else {
            let activity = self.create_process_activity(&config, None);
            processes.insert(
                name.clone(),
                ProcessEntry::new(config, activity, crate::ProcessStatus::waiting()),
            );
        }
        clear_stale_logs(&self.state_dir, &name);
        info!("Re-armed waiting process: {}", name);
        drop(processes);
        self.notify_lifecycle();
    }

    /// Mark a waiting process as stopped after its dependencies failed or were
    /// cancelled. The entry is kept so list/status/start still see the process
    /// and it can be started or re-armed later.
    pub async fn cancel_waiting(&self, name: &str) {
        let mut processes = self.processes.write().await;
        // Only a Waiting entry transitions; every other variant is reinserted
        // untouched so an entry can never vanish (dropping an Active entry
        // here would detach a live supervised child).
        match processes.get_mut(name) {
            Some(entry)
                if entry.status().transition
                    == Some(crate::StateTransition::WaitingForDependencies) =>
            {
                entry.activity.dependency_failed();
                entry.publish(crate::ProcessStatus::stopped(
                    crate::StopReason::DependencyFailure,
                    crate::ChildState::NeverSpawned,
                ));
                info!("Cancelled waiting process: {}", name);
                drop(processes);
                self.notify_lifecycle();
            }
            _ => {}
        }
    }

    /// Launch a previously registered waiting process.
    ///
    /// Transitions the `Waiting` entry to `Launching` under a single write
    /// lock, then awaits the detached settle task. The TUI elapsed time
    /// includes the waiting period since the activity was created at
    /// registration time.
    pub async fn launch_waiting(&self, name: &str) -> Result<Option<Arc<Job>>> {
        let settle = {
            let mut processes = self.processes.write().await;
            // Checked under the write lock so it serializes with stop_all's
            // post-cancel map reads: either this launch sees the cancelled
            // token and bails, or stop_all's drain observes the Launching
            // entry and waits for it to settle.
            if self.shutdown.is_cancelled() {
                bail!("process manager is shutting down");
            }
            match processes.get_mut(name) {
                Some(entry)
                    if entry.status().transition
                        == Some(crate::StateTransition::WaitingForDependencies) =>
                {
                    if !entry.config.start.enable {
                        info!("Registered auto start off process: {}", name);
                        entry.publish(crate::ProcessStatus::not_started());
                        drop(processes);
                        self.notify_lifecycle();
                        return Ok(None);
                    }
                    entry.start_launch();
                    // No await between the Launching insert and the settle
                    // spawn: the settle task always completes even if this
                    // caller is aborted mid-launch.
                    self.spawn_launch_settle(name.to_string())
                }
                Some(_) => bail!("Process {} is not in waiting state", name),
                None => bail!("Process {} not found", name),
            }
        };
        self.notify_lifecycle();
        Self::join_launch_settle(settle).await.map(Some)
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
        trace!("Starting command '{}': {}", config.name, config.exec);

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
            let mut processes = self.processes.write().await;
            // Checked under the write lock so it serializes with stop_all's
            // post-cancel map reads (see launch_waiting).
            if self.shutdown.is_cancelled() {
                bail!("process manager is shutting down");
            }
            info!("Registered auto start off process: {}", config.name);
            clear_stale_logs(&self.state_dir, &config.name);
            processes.insert(
                config.name.clone(),
                ProcessEntry::new(config, activity, crate::ProcessStatus::not_started()),
            );
            drop(processes);
            self.notify_lifecycle();
            return Ok(None);
        }

        let name = config.name.clone();
        let settle = {
            let mut processes = self.processes.write().await;
            // Checked under the write lock so it serializes with stop_all's
            // post-cancel map reads (see launch_waiting).
            if self.shutdown.is_cancelled() {
                bail!("process manager is shutting down");
            }
            let entry = ProcessEntry::new(config, activity, crate::ProcessStatus::waiting());
            entry.start_launch();
            processes.insert(name.clone(), entry);
            // No await between the Launching insert and the settle spawn: the
            // settle task always completes even if this caller is aborted.
            self.spawn_launch_settle(name)
        };
        self.notify_lifecycle();
        Self::join_launch_settle(settle).await.map(Some)
    }

    /// Await a detached launch settle task spawned by [`Self::spawn_launch_settle`].
    async fn join_launch_settle(settle: JoinHandle<Result<Arc<Job>>>) -> Result<Arc<Job>> {
        match settle.await {
            Ok(result) => result,
            Err(e) => bail!("process launch task failed: {}", e),
        }
    }

    /// Spawn the detached settle task for an entry already transitioned to
    /// `Launching`. Runs `launch_setup` and settles the entry under a single
    /// write lock: `Active` on success, `Stopped` on failure or when shutdown
    /// raced the launch (the spawned child is stopped before the entry leaves
    /// `Launching`). Detached so an aborted caller can never strand a
    /// `Launching` entry.
    fn spawn_launch_settle(&self, name: String) -> JoinHandle<Result<Arc<Job>>> {
        let processes = Arc::clone(&self.processes);
        let entries_changed = Arc::clone(&self.entries_changed);
        let task_notify = self.task_notify.clone();
        let shutdown = self.shutdown.clone();
        let state_dir = self.state_dir.clone();
        let live = Arc::clone(&self.live);
        let completion = Arc::clone(&self.completion);
        tokio::spawn(async move {
            let (config, activity_ref, notify_socket) = {
                let procs = processes.read().await;
                match procs.get(&name) {
                    Some(entry) if entry.is_launching() => (
                        entry.config.clone(),
                        entry.activity.ref_handle(),
                        entry.notify_socket.clone(),
                    ),
                    _ => bail!("process {} is not launching", name),
                }
            };

            let setup = Self::launch_setup(&state_dir, &config, &activity_ref, notify_socket).await;

            let mut procs = processes.write().await;
            match procs.get_mut(&name) {
                Some(entry) if entry.is_launching() => match setup {
                    Err(e) => {
                        entry.activity.fail();
                        entry.finish_launch_failure();
                        drop(procs);
                        notify_lifecycle_parts(&entries_changed, &task_notify);
                        Err(e)
                    }
                    Ok(setup) if shutdown.is_cancelled() => {
                        // Shutdown raced the launch: keep the entry Launching
                        // while the spawned child is stopped, so the map never
                        // reports the process gone or stopped before the child
                        // is dead. Bounded by the stop grace period.
                        drop(procs);
                        setup.abort_and_stop().await;
                        let mut procs = processes.write().await;
                        if let Some(entry) = procs.get_mut(&name)
                            && entry.is_launching()
                        {
                            entry.publish(crate::ProcessStatus::stopped(
                                crate::StopReason::ManagerShutdown,
                                crate::ChildState::Terminated,
                            ));
                        }
                        drop(procs);
                        notify_lifecycle_parts(&entries_changed, &task_notify);
                        bail!("process manager is shutting down")
                    }
                    Ok(setup) => {
                        entry.publish(initial_status(&entry.config));
                        let status_rx = entry.status_rx.clone();
                        let status_tx = entry.status_tx.clone();
                        entry.notify_socket = setup.notify_socket.clone();
                        let resources = ProcessResources {
                            config: entry.config.clone(),
                            job: setup.job.clone(),
                            activity: entry.activity.ref_handle(),
                            notify_socket: setup.notify_socket.clone(),
                            status_tx,
                            stderr_log: setup.stderr_log,
                            scopes: setup.scopes,
                            live: Arc::clone(&live),
                            completion: Arc::clone(&completion),
                            stop_requested: CancellationToken::new(),
                        };
                        let (cmd_tx, cmd_rx) = mpsc::channel(8);
                        let supervisor_task = crate::supervisor::spawn_supervisor(
                            &resources,
                            shutdown.clone(),
                            cmd_rx,
                        );
                        let notify_forwarder = spawn_notify_forwarder(
                            task_notify.clone(),
                            Arc::clone(&entries_changed),
                            status_rx.clone(),
                        );
                        entry.handle = Some(JobHandle {
                            resources,
                            status_rx,
                            cmd_tx,
                            supervisor_task,
                            output_readers: Some((setup.stdout_tailer, setup.stderr_tailer)),
                            notify_forwarder,
                        });
                        drop(procs);
                        notify_lifecycle_parts(&entries_changed, &task_notify);
                        info!("Command '{}' started", name);
                        Ok(setup.job)
                    }
                },
                _ => {
                    // Unreachable given every other path refuses to touch a
                    // Launching entry; defensive so a spawned child can never
                    // detach from the map.
                    drop(procs);
                    if let Ok(setup) = setup {
                        setup.abort_and_stop().await;
                    }
                    bail!("process {} entry changed during launch", name)
                }
            }
        })
    }

    /// Set up everything a launch produces before the entry settles to
    /// `Active`: probes, sockets, command, job start, and log tailers.
    async fn launch_setup(
        state_dir: &Path,
        config: &ProcessConfig,
        activity: &devenv_activity::ActivityRef,
        existing_notify_socket: Option<Arc<NotifySocket>>,
    ) -> Result<LaunchSetup> {
        // A previous manager may have left this process's scope behind.
        let claim =
            crate::process_guardian::recover_and_claim_process(state_dir, &config.name).await?;

        let supervise_locally = config.supervisor == crate::config::SupervisionMode::Native;
        let uses_notify = supervise_locally && config.ready.as_ref().is_some_and(|r| r.notify);
        let notify_socket = if uses_notify {
            if let Some(socket) = existing_notify_socket {
                socket.drain()?;
                Some(socket)
            } else {
                let socket = Arc::new(NotifySocket::new(state_dir, &config.name).await?);
                info!(
                    "Created notify socket for {} at {}",
                    config.name,
                    socket.path().display()
                );
                Some(socket)
            }
        } else {
            None
        };

        let watchdog_usec = supervise_locally
            .then(|| config.watchdog.as_ref().map(|w| w.usec))
            .flatten();

        // Build the command (creates log directory and wrapper script)
        let proc_cmd = crate::command::build_command(
            state_dir,
            config,
            notify_socket.as_ref().map(|s| s.path()),
            watchdog_usec,
        )?;

        // Truncate log files if they exist
        let _ = std::fs::write(&proc_cmd.stdout_log, "");
        let _ = std::fs::write(&proc_cmd.stderr_log, "");

        let (job, _task) = start_job(proc_cmd.command);
        let job = Arc::new(job);
        let scopes = Arc::new(crate::process_guardian::ProcessScopeRegistry::default());

        // watchexec-supervisor reports spawn failures only through its error
        // handler. Without one, the start ticket completes successfully even
        // when no child was created.
        let (start_error_tx, mut start_error_rx) = mpsc::unbounded_channel();
        let process_name = config.name.clone();
        job.set_error_handler(move |error| {
            let message = error
                .get()
                .map(ToString::to_string)
                .unwrap_or_else(|| "unknown supervisor error".to_string());
            warn!("Process '{}' supervisor error: {}", process_name, message);
            let _ = start_error_tx.send(message);
        })
        .await;

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
        let registration = crate::process_guardian::ProcessScopeRegistrationWrapper {
            state_dir: state_dir.to_path_buf(),
            process_name: config.name.clone(),
            shutdown: config.shutdown.clone(),
            registry: Arc::clone(&scopes),
            _claim: claim.clone(),
            prepared_scope: None,
            spawned_scope: None,
        };

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

            // Inject OTEL trace context so instrumented subprocesses join the trace.
            cmd.envs(devenv_activity::trace_propagation_env());

            // Record the scope so a force exit, which skips both teardown and
            // destructors, can still reach processes that would otherwise be
            // orphaned to init.
            command_wrap.wrap(registration.clone());

            if let Some((ref fds, ref capabilities)) = process_setup {
                command_wrap.wrap(ProcessSetupWrapper::new(fds.clone(), capabilities.clone()));
            }
        });

        job.start().await;
        if let Ok(error) = start_error_rx.try_recv() {
            job.delete().await;
            bail!("Failed to spawn process '{}': {}", config.name, error);
        }
        // Spawn file tailers to emit output to activity
        let stderr_log = proc_cmd.stderr_log.clone();
        let stdout_tailer =
            crate::log_tailer::spawn_file_tailer(proc_cmd.stdout_log, activity.clone(), false);
        let stderr_tailer =
            crate::log_tailer::spawn_file_tailer(proc_cmd.stderr_log, activity.clone(), true);

        Ok(LaunchSetup {
            job,
            notify_socket,
            stdout_tailer,
            stderr_tailer,
            stderr_log,
            shutdown: config.shutdown.clone(),
            scopes,
        })
    }

    /// Shared teardown for [`Self::stop`] and [`Self::stop_and_keep`] once the
    /// `Active` handle has been extracted and a `Stopped` placeholder inserted
    /// under the write lock: abort the supervisor, forwarder, and output
    /// readers, signal the child with the grace period, wait for declared ports
    /// to be released, then mark the activity `Stopped`.
    async fn finish_stop(&self, name: &str, parts: StopParts) {
        let StopParts {
            job,
            cmd_tx,
            supervisor_task,
            notify_forwarder,
            output_readers,
            ports,
            shutdown,
            scopes,
            reason,
        } = parts;

        stop_via_supervisor(&cmd_tx, supervisor_task).await;
        notify_forwarder.abort();

        if let Some((stdout_reader, stderr_reader)) = output_readers {
            stdout_reader.abort();
            stderr_reader.abort();
        }

        crate::process_guardian::stop_job(&job, &scopes, &shutdown).await;

        if !ports.is_empty() {
            let release_status =
                wait_for_port_conflicts_to_settle(&ports, PORT_RELEASE_TIMEOUT).await;

            if !release_status.ownerless_ports().is_empty() {
                let port_list = release_status
                    .ownerless_ports()
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                debug!(
                    "Ports still in transient ownerless teardown after stopping {}: {}",
                    name, port_list
                );
            }

            if !release_status.blocking_ports().is_empty() {
                let port_list = release_status
                    .blocking_ports()
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                let details = release_status
                    .owned
                    .iter()
                    .map(|(port, owner)| format!("{}{}", port, owner))
                    .chain(
                        release_status
                            .unknown
                            .iter()
                            .map(|(port, reason)| format!("{} ({})", port, reason)),
                    )
                    .collect::<Vec<_>>()
                    .join(", ");
                warn!(
                    "Ports still busy after {:.1}s for process {}: {}",
                    PORT_RELEASE_TIMEOUT.as_secs_f32(),
                    name,
                    port_list
                );
                debug!("Port release blockers for {}: {}", name, details);
            }
        }

        // Publish Stopped only after child/scope cleanup and port settling.
        let mut processes = self.processes.write().await;
        if let Some(entry) = processes.get_mut(name) {
            entry.finish_controlled_stop(reason);
        }
        drop(processes);

        self.notify_lifecycle();
        info!("Process {} stopped", name);
    }

    /// Stop a process by name. An explicit stop is *displayed* as a plain
    /// `Stopped` (the user's final word) even if the process had already
    /// exited, but a dependent on `<proc>@started` still sees that it ran.
    pub async fn stop(&self, name: &str) -> Result<()> {
        self.stop_inner(name, true).await
    }

    /// Stop a process by name. `user_stopped` marks an explicit user stop
    /// (`devenv processes stop`, Ctrl-X) vs. shutdown teardown (`stop_all`):
    /// the former is displayed as a plain `Stopped`, the latter keeps the
    /// terminal phase. Either way the terminal phase is recorded, so dependents
    /// still observe that the process ran.
    async fn stop_inner(&self, name: &str, user_stopped: bool) -> Result<()> {
        // Keep a Stopping entry visible throughout teardown.
        let parts = {
            let mut processes = self.processes.write().await;

            let entry = processes
                .get_mut(name)
                .ok_or_else(|| miette::miette!("Process {} not found", name))?;
            let Some(handle) = entry.handle.take() else {
                let state = match entry.status().display_phase() {
                    ProcessPhase::NotStarted => "auto start off",
                    ProcessPhase::Stopping => "stopping",
                    ProcessPhase::Stopped | ProcessPhase::Exited | ProcessPhase::GaveUp => {
                        "already stopped"
                    }
                    ProcessPhase::Waiting => "waiting for dependencies",
                    ProcessPhase::Starting => "starting",
                    ProcessPhase::Ready => "not supervised",
                };
                bail!("Process {} is {}, cannot stop", name, state)
            };
            take_active_for_stop(entry, handle, user_stopped)
        };

        trace!("Stopping process: {}", name);

        self.notify_lifecycle();
        self.finish_stop(name, parts).await;
        Ok(())
    }

    /// Stop a running process but keep its entry in the process map so the TUI
    /// continues to show it and the user can restart it with Ctrl+R.
    ///
    /// Transitions an `Active` entry to `ProcessEntry::Stopped { .. }` — a
    /// distinct variant from `NotStarted` so callers of [`Self::get_phase`]
    /// can tell apart a process the user stopped from one that never started.
    /// Errors if the process is not currently `Active`.
    ///
    /// Identical to [`Self::stop`]: both keep the entry visible as `Stopped`
    /// (the user's final word) while still recording the terminal phase for
    /// dependents. Kept as a distinct, intent-revealing name for the Ctrl-X
    /// path that wants the process to remain restartable.
    pub async fn stop_and_keep(&self, name: &str) -> Result<()> {
        self.stop_inner(name, true).await
    }

    /// Signal all supervisors to shut down gracefully.
    ///
    /// This wakes the supervisor loops so they exit before we abort their tasks.
    pub fn shutdown_supervisors(&self) {
        self.shutdown.cancel();
    }

    /// Stop all active processes, draining in-flight launches first.
    ///
    /// Entries are never removed: stopped processes keep a `Stopped` entry
    /// (with any terminal phase preserved) so run summaries and API queries
    /// still see them after teardown.
    pub async fn stop_all(&self) -> Result<()> {
        trace!("stop_all: shutting down supervisors");
        // Cancelling the token also blocks new launches and makes in-flight
        // launch settles transition their Launching entries to Stopped.
        self.shutdown_supervisors();

        loop {
            let notified = self.entries_changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            let names = active_names(&*self.processes.read().await);
            if !names.is_empty() {
                trace!("stop_all: stopping {} processes: {:?}", names.len(), names);
                for (name, result) in names.iter().zip(
                    futures::future::join_all(
                        // Shutdown teardown (not a user stop): the preserved
                        // self-exit/give-up phase is displayed as-is, so the run
                        // summary still reflects how each process ended.
                        names.iter().map(|name| self.stop_inner(name, false)),
                    )
                    .await,
                ) {
                    if let Err(err) = result {
                        warn!("Failed to stop process {}: {}", name, err);
                    }
                }
                continue;
            }

            let teardown_in_flight = self.processes.read().await.values().any(|entry| {
                matches!(
                    entry.status().transition,
                    Some(crate::StateTransition::Launching | crate::StateTransition::Terminating)
                )
            });
            if !teardown_in_flight {
                break;
            }
            // Both paths publish a final map transition.
            notified.await;
        }

        // Manager shutdown ends the stable-socket lifetime. Ordinary user
        // stop/start cycles retain it; final shutdown releases the last owner
        // so the filesystem entry is cleaned up.
        for entry in self.processes.write().await.values_mut() {
            entry.notify_socket = None;
        }

        Ok(())
    }

    /// Restart a process by name
    ///
    /// This resets the restart count and activity state, respawns the supervision
    /// task if it exited (e.g., due to max restarts), and restarts the underlying job.
    pub async fn restart(&self, name: &str) -> Result<RestartOutcome> {
        let mut processes = self.processes.write().await;
        // Checked under the write lock so it serializes with stop_all's
        // post-cancel map reads (see launch_waiting).
        if self.shutdown.is_cancelled() {
            bail!("process manager is shutting down");
        }
        let entry = processes
            .get_mut(name)
            .ok_or_else(|| miette::miette!("Process {} not running", name))?;
        let handle = match entry.handle.as_mut() {
            Some(handle) => handle,
            None => match entry.status().transition {
                Some(crate::StateTransition::Terminating) => {
                    bail!("Process {} is stopping", name)
                }
                Some(crate::StateTransition::WaitingForDependencies) => {
                    bail!("Process {} is waiting for dependencies", name)
                }
                Some(crate::StateTransition::Launching | crate::StateTransition::Replacing) => {
                    bail!("Process {} is starting", name)
                }
                None if entry.status().child == crate::ChildState::Running => {
                    bail!("Process {} is not supervised", name)
                }
                None => return Ok(RestartOutcome::SchedulingRequired),
            },
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
                handle.resources.activity.clone(),
                false,
            ),
            crate::log_tailer::spawn_file_tailer(
                stderr_log,
                handle.resources.activity.clone(),
                true,
            ),
        ));

        // Let a live supervisor serialize the restart with exit handling.
        let driven_by_existing = if handle.supervisor_task.is_finished() {
            false
        } else {
            let (ack_tx, ack_rx) = oneshot::channel();
            let sent = handle
                .cmd_tx
                .send(crate::supervisor::SupervisorCommand::Restart { ack: ack_tx })
                .await
                .is_ok();
            sent && ack_rx.await.is_ok()
        };

        if !driven_by_existing {
            info!(
                "Supervisor for {} no longer monitors the job, starting fresh with new supervision",
                name
            );
            let old_task = std::mem::replace(&mut handle.supervisor_task, tokio::spawn(async {}));
            let _ = old_task.await;
            if !crate::process_guardian::restart_job(
                &handle.resources.job,
                &handle.resources.scopes,
                &handle.resources.config.shutdown,
                &self.shutdown,
                &handle.resources.stop_requested,
                handle.resources.notify_socket.as_deref(),
            )
            .await?
            {
                bail!("process manager is shutting down");
            }
            let status = initial_status(&handle.resources.config);
            let _ = handle.resources.status_tx.send(status);
            handle
                .resources
                .activity
                .set_status(status.activity_status());
            let (cmd_tx, cmd_rx) = mpsc::channel(8);
            handle.supervisor_task = crate::supervisor::spawn_supervisor(
                &handle.resources,
                self.shutdown.clone(),
                cmd_rx,
            );
            handle.cmd_tx = cmd_tx;
        }

        info!("Process {} restarted", name);
        Ok(RestartOutcome::RestartedInPlace)
    }

    /// Start a previously not-started or stopped process, reusing its existing TUI activity.
    pub async fn start_not_started(&self, name: &str) -> Result<Arc<Job>> {
        let settle = {
            let mut processes = self.processes.write().await;
            // Checked under the write lock so it serializes with stop_all's
            // post-cancel map reads (see launch_waiting).
            if self.shutdown.is_cancelled() {
                bail!("process manager is shutting down");
            }
            let entry = processes
                .get_mut(name)
                .ok_or_else(|| miette::miette!("Process {} not found", name))?;
            if entry.status().transition == Some(crate::StateTransition::Terminating) {
                bail!("Process {} is stopping", name);
            }
            if !entry.can_prepare_start()
                || entry.status().transition == Some(crate::StateTransition::WaitingForDependencies)
            {
                bail!("Process {} is already running", name);
            }

            entry.activity.reset();
            entry.start_launch();

            info!("Starting not-started process: {}", name);
            // No await between the Launching insert and the settle spawn: the
            // settle task always completes even if this caller is aborted.
            self.spawn_launch_settle(name.to_string())
        };
        self.notify_lifecycle();
        Self::join_launch_settle(settle).await
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
            processes
                .get(name)
                .ok_or_else(|| miette::miette!("Process {} not found", name))?
                .status_rx
                .clone()
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

    /// Wait until a launch has either reached usable readiness or a terminal
    /// fact makes readiness impossible. Returns the coherent terminal status.
    pub async fn wait_launch_settled(
        &self,
        name: &str,
        cancel: &CancellationToken,
    ) -> Result<crate::ProcessStatus> {
        let mut status_rx = {
            let processes = self.processes.read().await;
            processes
                .get(name)
                .ok_or_else(|| miette::miette!("Process {} not found", name))?
                .status_rx
                .clone()
        };

        loop {
            let status = *status_rx.borrow_and_update();
            if status.is_ready()
                || (status.transition.is_none() && status.child != crate::ChildState::Running)
            {
                return Ok(status);
            }

            tokio::select! {
                changed = status_rx.changed() => {
                    if changed.is_err() {
                        bail!("Process {} launch status channel closed", name);
                    }
                }
                _ = cancel.cancelled() => {
                    bail!("Process {} launch wait cancelled", name);
                }
            }
        }
    }

    /// Query the current state of a process.
    pub async fn job_state(&self, name: &str) -> Option<crate::supervisor_state::JobStatus> {
        let processes = self.processes.read().await;
        processes.get(name).map(|entry| entry.status())
    }

    /// Keep a transient process runner alive until cancellation or idleness.
    /// Interactive commands and dependency-aware restarts belong to the
    /// persistent manager in `devenv-tasks`.
    pub async fn run_until(
        &self,
        cancellation_token: CancellationToken,
        mode: OnIdle,
    ) -> Result<()> {
        let done = || mode == OnIdle::Exit && self.live.load(Ordering::SeqCst) == 0;
        if done() {
            return Ok(());
        }

        loop {
            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    self.stop_all().await?;
                    break;
                }
                _ = self.completion.notified() => {}
            }

            if done() {
                break;
            }
        }

        Ok(())
    }
}

impl NativeManagerClient {
    /// Connect to a running manager and open an attach event stream.
    pub async fn attach_stream(socket_path: &Path) -> Result<AttachStream> {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let stream = Self::send_api_request(socket_path, &ApiRequest::Attach).await?;

        // The reader task owns the socket so read_line's cancel-unsafety is
        // contained; the consumer can select! on next() safely.
        let (tx, rx) = mpsc::channel::<Result<AttachEvent>>(256);
        let reader_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
                let event = match serde_json::from_str::<AttachEvent>(&line) {
                    Ok(event) => Ok(event),
                    // An older daemon answers Attach with a one-shot
                    // ApiResponse::Error ("unknown variant"); surface it as
                    // the stream error.
                    Err(_) => match serde_json::from_str::<ApiResponse>(&line) {
                        Ok(ApiResponse::Error { message }) => Err(miette::miette!("{}", message)),
                        _ => Err(miette::miette!(
                            "unexpected attach response: {}",
                            line.trim_end()
                        )),
                    },
                };
                if tx.send(event).await.is_err() {
                    return;
                }
            }
        });

        Ok(AttachStream { rx, reader_task })
    }

    /// Connect to a running native manager's API socket and send a request.
    pub async fn api_request(socket_path: &Path, request: &ApiRequest) -> Result<ApiResponse> {
        let stream = Self::send_api_request(socket_path, request).await?;
        Self::read_api_response(stream).await
    }

    /// Ask where a running manager resides. `None` means the manager could not
    /// be reached or did not answer (e.g. a daemon predating the residence
    /// request); callers treat that as `Daemon` for backward compatibility. A
    /// live in-process manager always answers `InProcess`, so it cannot be
    /// misread.
    pub async fn query_manager_residence(socket_path: &Path) -> Option<ManagerResidence> {
        match Self::api_request(socket_path, &ApiRequest::Residence).await {
            Ok(ApiResponse::Residence { residence }) => Some(residence),
            _ => None,
        }
    }

    /// One-shot request whose reply legitimately takes as long as the work it
    /// triggers (the daemon answers `Start` only after the full task DAG and
    /// process launches complete): only the connect/send phase is bounded,
    /// the reply read is unbounded and callers race it against cancellation.
    pub async fn api_request_bounded_connect(
        socket_path: &Path,
        request: &ApiRequest,
        connect_timeout: Duration,
    ) -> Result<ApiResponse> {
        let stream = tokio::time::timeout(
            connect_timeout,
            Self::send_api_request(socket_path, request),
        )
        .await
        .map_err(|_| miette::miette!("timed out connecting to the process manager"))??;
        Self::read_api_response(stream).await
    }

    /// Connect to the manager socket and write one JSON request line,
    /// returning the stream positioned to read the reply.
    async fn send_api_request(
        socket_path: &Path,
        request: &ApiRequest,
    ) -> Result<tokio::net::UnixStream> {
        use tokio::io::AsyncWriteExt;

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

        Ok(stream)
    }

    /// Read the single JSON response line of a one-shot request.
    async fn read_api_response(stream: tokio::net::UnixStream) -> Result<ApiResponse> {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let mut reader = BufReader::new(stream);
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
            other => bail!("Unexpected response: {:?}", other),
        }
    }
}

#[async_trait]
impl ProcessManagerControl for NativeManagerClient {
    async fn stop(&self) -> Result<()> {
        let manager_pid_file = self.manager_pid_file();
        if !manager_pid_file.exists() {
            bail!("Native process manager not running (PID file not found)");
        }

        let manager_pid = std::fs::read_to_string(&manager_pid_file)
            .into_diagnostic()?
            .trim()
            .parse::<u32>()
            .into_diagnostic()?;
        let pid = Pid::from_raw(manager_pid as i32);

        info!("Stopping native process manager (PID: {})", manager_pid);

        match signal::kill(pid, NixSignal::SIGTERM) {
            Ok(()) => {
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
            Err(error) => {
                bail!(
                    "Failed to send SIGTERM to manager process (PID {}): {}",
                    pid,
                    error
                );
            }
        }

        // Wait for shutdown with exponential backoff.
        let start = std::time::Instant::now();
        let max_wait = Duration::from_secs(30);
        let mut interval = Duration::from_millis(100);
        let max_interval = Duration::from_secs(1);

        loop {
            match signal::kill(pid, None) {
                Ok(()) => {
                    if start.elapsed() >= max_wait {
                        warn!(
                            "Manager did not shut down within {} seconds, sending SIGKILL",
                            max_wait.as_secs()
                        );

                        match signal::kill(pid, NixSignal::SIGKILL) {
                            Ok(()) => info!("Sent SIGKILL to manager (PID {})", pid),
                            Err(error) => warn!("Failed to send SIGKILL: {}", error),
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
                Err(error) => {
                    warn!("Error checking manager process: {}", error);
                    break;
                }
            }
        }

        // The daemon may already have removed its PID file.
        let _ = tokio::fs::remove_file(&manager_pid_file).await;

        info!("Native process manager stopped");
        Ok(())
    }

    async fn is_running(&self) -> bool {
        matches!(
            pid::check_pid_file(&self.manager_pid_file()).await,
            Ok(PidStatus::Running(_))
        )
    }
}

impl Drop for ProcessRunner {
    fn drop(&mut self) {
        // A runner owns only the child supervisors it created. Persistent
        // discovery state is owned by the API server and manager host.
        self.shutdown_supervisors();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ListenKind, ListenSpec, RestartPolicy, StartConfig};
    use std::net::Ipv4Addr;

    static ACTIVITY_EVENT_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn saw_process_status(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<devenv_activity::ActivityEvent>,
        process_name: &str,
        expected: ProcessStatus,
    ) -> bool {
        let mut process_id = None;
        while let Ok(event) = rx.try_recv() {
            if let devenv_activity::ActivityEvent::Process(event) = event {
                match event {
                    devenv_activity::Process::Start { id, name, .. } if name == process_name => {
                        process_id = Some(id);
                    }
                    devenv_activity::Process::Status { id, status, .. }
                        if Some(id) == process_id && status == expected =>
                    {
                        return true;
                    }
                    _ => {}
                }
            }
        }
        false
    }

    #[tokio::test]
    async fn test_create_manager() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf());
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

        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();

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

    async fn wait_for_manager_phase(manager: &ProcessRunner, name: &str, expected: ProcessPhase) {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let notified = manager.entries_changed().notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if manager.get_phase(name).await == Some(expected) {
                    return;
                }
                notified.await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {name} to reach {expected}"));
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
            exec: "exec tail -f /dev/null".to_string(),
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
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();
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
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();

        assert_eq!(manager.get_phase("nonexistent").await, None);
    }

    #[tokio::test]
    async fn test_cancel_waiting_marks_stopped() {
        let _activity_test_guard = ACTIVITY_EVENT_TEST_LOCK.lock().await;
        let (mut rx, handle) = devenv_activity::init();
        let _activity_guard = handle.install();

        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();
        let config = test_config("cancel-me");

        manager.register_waiting(config, None).await;
        assert_eq!(
            manager.get_phase("cancel-me").await,
            Some(ProcessPhase::Waiting)
        );

        manager.cancel_waiting("cancel-me").await;
        assert_eq!(
            manager.get_phase("cancel-me").await,
            Some(ProcessPhase::Stopped)
        );
        assert!(manager.list().await.is_empty());
        assert!(
            saw_process_status(&mut rx, "cancel-me", ProcessStatus::Stopped),
            "cancelling a waiting process must immediately emit Stopped"
        );
    }

    #[tokio::test]
    async fn test_launch_failure_marks_stopped() {
        let _activity_test_guard = ACTIVITY_EVENT_TEST_LOCK.lock().await;
        let (mut rx, handle) = devenv_activity::init();
        let _activity_guard = handle.install();

        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();
        // An unparsable TCP listen address makes `activation_from_listen`
        // fail inside launch_setup.
        let config = ProcessConfig {
            listen: vec![ListenSpec {
                name: "bad".to_string(),
                kind: ListenKind::Tcp,
                address: Some("not-an-address".to_string()),
                path: None,
                backlog: None,
                mode: None,
            }],
            ..test_config("fail-launch")
        };

        manager.register_waiting(config, None).await;
        let result = manager.launch_waiting("fail-launch").await;

        assert!(
            result.is_err(),
            "launch must fail on an invalid listen spec"
        );
        assert_eq!(
            manager.get_phase("fail-launch").await,
            Some(ProcessPhase::Stopped),
            "failed launch must keep a Stopped entry, not vanish"
        );
        assert!(
            saw_process_status(&mut rx, "fail-launch", ProcessStatus::Stopped),
            "failed launch setup must immediately emit Stopped"
        );
    }

    #[tokio::test]
    async fn test_supervisor_transitions_fire_task_notify() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();

        let notify = Arc::new(Notify::new());
        manager.set_task_notify(notify.clone());

        manager
            .start_command(&test_config("short-lived"), None)
            .await
            .unwrap();

        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                let notified = notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if manager.get_phase("short-lived").await == Some(ProcessPhase::Exited) {
                    break;
                }
                notified.await;
            }
        })
        .await
        .expect("task_notify should fire on supervisor transitions until Exited");

        let _ = manager.stop_all().await;
    }

    #[tokio::test]
    async fn explicit_stop_after_self_exit_reports_stopped() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();
        let notify = Arc::new(Notify::new());
        manager.set_task_notify(notify.clone());

        manager
            .start_command(&test_config("self-exit"), None)
            .await
            .unwrap();

        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                let notified = notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if manager.get_phase("self-exit").await == Some(ProcessPhase::Exited) {
                    break;
                }
                notified.await;
            }
        })
        .await
        .expect("process should exit on its own");

        manager.stop_and_keep("self-exit").await.unwrap();

        assert_eq!(
            manager.get_phase("self-exit").await,
            Some(ProcessPhase::Stopped),
            "an explicit stop must report Stopped, not the preserved Exited phase"
        );

        let _ = manager.stop_all().await;
    }

    #[tokio::test]
    async fn restart_returns_scheduling_required_for_a_stopped_process() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();

        let mut config = test_config("restart-scheduling");
        config.exec = "exec tail -f /dev/null".to_string();
        manager.start_command(&config, None).await.unwrap();
        manager.stop_and_keep("restart-scheduling").await.unwrap();

        assert_eq!(
            manager.restart("restart-scheduling").await.unwrap(),
            RestartOutcome::SchedulingRequired
        );

        manager.stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn self_exit_updates_activity_status_to_exited() {
        let _activity_test_guard = ACTIVITY_EVENT_TEST_LOCK.lock().await;

        let (mut rx, handle) = devenv_activity::init();
        let _guard = handle.install();

        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();

        manager
            .start_command(&test_config("self-exit"), None)
            .await
            .unwrap();

        let saw_exited = tokio::time::timeout(Duration::from_secs(60), async {
            let mut proc_id: Option<u64> = None;
            while let Some(event) = rx.recv().await {
                if let devenv_activity::ActivityEvent::Process(p) = event {
                    match p {
                        devenv_activity::Process::Start { id, name, .. } if name == "self-exit" => {
                            proc_id = Some(id);
                        }
                        devenv_activity::Process::Status { id, status, .. }
                            if Some(id) == proc_id && status == ProcessStatus::Exited =>
                        {
                            return true;
                        }
                        _ => {}
                    }
                }
            }
            false
        })
        .await
        .expect("timed out waiting for the self-exit process's activity status");
        assert!(
            saw_exited,
            "a self-exited process must emit a terminal Exited activity status"
        );

        let _ = manager.stop_all().await;
    }

    #[tokio::test]
    async fn exhausted_restart_budget_updates_activity_status_to_gave_up() {
        let _activity_test_guard = ACTIVITY_EVENT_TEST_LOCK.lock().await;
        let (mut rx, handle) = devenv_activity::init();
        let _guard = handle.install();

        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();
        let config = ProcessConfig {
            name: "crash-loop".to_string(),
            exec: "false".to_string(),
            restart: crate::config::RestartConfig {
                on: RestartPolicy::OnFailure,
                max: Some(0),
                window: None,
            },
            ..Default::default()
        };

        manager.start_command(&config, None).await.unwrap();

        let saw_gave_up = tokio::time::timeout(Duration::from_secs(60), async {
            let mut proc_id = None;
            while let Some(event) = rx.recv().await {
                if let devenv_activity::ActivityEvent::Process(process) = event {
                    match process {
                        devenv_activity::Process::Start { id, name, .. }
                            if name == "crash-loop" =>
                        {
                            proc_id = Some(id);
                        }
                        devenv_activity::Process::Status { id, status, .. }
                            if Some(id) == proc_id && status == ProcessStatus::GaveUp =>
                        {
                            return true;
                        }
                        _ => {}
                    }
                }
            }
            false
        })
        .await
        .expect("timed out waiting for the crash-loop activity status");

        assert!(saw_gave_up, "GaveUp must not be collapsed to Stopped");
        assert_eq!(
            manager.get_phase("crash-loop").await,
            Some(ProcessPhase::GaveUp)
        );

        let _ = manager.stop_all().await;
    }

    #[tokio::test]
    async fn test_cancel_waiting_noop_for_unknown() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();

        manager.cancel_waiting("does-not-exist").await;
    }

    #[tokio::test]
    async fn test_launch_waiting_auto_start_off_becomes_not_started() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();
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
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();

        let result = manager.launch_waiting("ghost").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_launch_waiting_not_in_waiting_state_errors() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();
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
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();
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
    async fn test_rearm_waiting_relaunches_stopped_process() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();
        let config = long_running_config("relaunch-me");

        manager.register_waiting(config.clone(), None).await;
        manager.launch_waiting("relaunch-me").await.unwrap();
        manager.stop_and_keep("relaunch-me").await.unwrap();
        assert_eq!(
            manager.get_phase("relaunch-me").await,
            Some(ProcessPhase::Stopped)
        );

        manager.rearm_waiting(config).await;
        assert_eq!(
            manager.get_phase("relaunch-me").await,
            Some(ProcessPhase::Waiting)
        );
        let job = manager.launch_waiting("relaunch-me").await.unwrap();
        assert!(job.is_some(), "stopped process should relaunch");
        assert_ne!(
            manager.get_phase("relaunch-me").await,
            Some(ProcessPhase::Stopped)
        );

        let _ = manager.stop_all().await;
    }

    #[tokio::test]
    async fn test_rearm_waiting_clears_stale_logs() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();
        let name = "rearm-logs";

        manager
            .register_waiting(auto_start_off_config(name), None)
            .await;
        manager.launch_waiting(name).await.unwrap();
        assert_eq!(
            manager.get_phase(name).await,
            Some(ProcessPhase::NotStarted)
        );

        let (stdout_log, stderr_log) = crate::command::log_paths(temp_dir.path(), name);
        std::fs::create_dir_all(stdout_log.parent().unwrap()).unwrap();
        std::fs::write(&stdout_log, "old stdout\n").unwrap();
        std::fs::write(&stderr_log, "old stderr\n").unwrap();

        manager.rearm_waiting(test_config(name)).await;

        assert_eq!(manager.get_phase(name).await, Some(ProcessPhase::Waiting));
        assert_eq!(std::fs::read_to_string(&stdout_log).unwrap(), "");
        assert_eq!(std::fs::read_to_string(&stderr_log).unwrap(), "");

        let _ = manager.stop_all().await;
    }

    #[tokio::test]
    async fn test_launch_waiting_notifies_task_system() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();

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
    async fn test_start_processes_preserves_process_env_over_global_env() {
        let temp_dir = tempfile::tempdir().unwrap();
        let _manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();

        // Process config with a per-process env var
        let mut config = ProcessConfig {
            name: "env-test".to_string(),
            exec: "env".to_string(),
            args: vec![],
            restart: crate::config::RestartConfig {
                on: RestartPolicy::Never,
                max: Some(0),
                window: None,
            },
            env: HashMap::from([
                ("SHARED_VAR".to_string(), "per-process".to_string()),
                ("PROCESS_ONLY".to_string(), "yes".to_string()),
            ]),
            ..Default::default()
        };

        // Global env that also defines SHARED_VAR
        let global_env: HashMap<String, String> = HashMap::from([
            ("SHARED_VAR".to_string(), "global".to_string()),
            ("GLOBAL_ONLY".to_string(), "yes".to_string()),
        ]);

        // Simulate the merging logic from start_processes
        let mut merged_env = global_env.clone();
        merged_env.extend(config.env.clone());
        config.env = merged_env;

        // Per-process value must win
        assert_eq!(config.env.get("SHARED_VAR").unwrap(), "per-process");
        // Both sources should be present
        assert_eq!(config.env.get("PROCESS_ONLY").unwrap(), "yes");
        assert_eq!(config.env.get("GLOBAL_ONLY").unwrap(), "yes");
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

        let status = wait_for_port_conflicts_to_settle(&[port], Duration::from_secs(1)).await;

        assert!(
            status.blocking_ports().is_empty() && status.ownerless_ports().is_empty(),
            "port should have been released"
        );
        assert!(
            started.elapsed() >= Duration::from_millis(150),
            "expected to wait for the listener to close"
        );
    }

    #[tokio::test]
    async fn test_wait_for_port_release_returns_early_for_ownerless_conflicts() {
        let mut probes = 0;

        let status = wait_for_port_conflicts_to_settle_with(
            &[6380],
            Duration::from_secs(1),
            |_| {
                probes += 1;
                if probes < 3 {
                    PortReleaseState::Owned(" (PID 123)".to_string())
                } else {
                    PortReleaseState::Ownerless
                }
            },
            |_| std::future::ready(()),
        )
        .await;

        assert_eq!(
            probes, 3,
            "should stop once only ownerless conflicts remain"
        );
        assert!(status.blocking_ports().is_empty());
        assert_eq!(status.ownerless_ports(), &[6380]);
    }

    #[tokio::test]
    async fn test_wait_for_port_release_times_out_for_owned_conflicts() {
        let started = std::time::Instant::now();

        let status = wait_for_port_conflicts_to_settle_with(
            &[6380],
            Duration::from_millis(20),
            |_| PortReleaseState::Owned(" (PID 123)".to_string()),
            |_| tokio::time::sleep(Duration::from_millis(2)),
        )
        .await;

        assert!(
            started.elapsed() >= Duration::from_millis(20),
            "owned conflicts should keep waiting until timeout"
        );
        assert_eq!(status.blocking_ports(), vec![6380]);
        assert!(status.ownerless_ports().is_empty());
    }

    #[tokio::test]
    async fn test_wait_for_port_release_times_out_for_unknown_conflicts() {
        let started = std::time::Instant::now();

        let status = wait_for_port_conflicts_to_settle_with(
            &[6380],
            Duration::from_millis(20),
            |_| PortReleaseState::Unknown("socket inspection failed".to_string()),
            |_| tokio::time::sleep(Duration::from_millis(2)),
        )
        .await;

        assert!(
            started.elapsed() >= Duration::from_millis(20),
            "unknown conflicts should keep waiting until timeout"
        );
        assert_eq!(status.blocking_ports(), vec![6380]);
        assert_eq!(
            status.unknown,
            vec![(6380, "socket inspection failed".to_string())]
        );
        assert!(status.ownerless_ports().is_empty());
    }

    #[tokio::test]
    async fn test_stop_and_keep_transitions_to_stopped() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();
        let config = long_running_config("keepable");

        manager.start_command(&config, None).await.unwrap();
        assert!(manager.list().await.contains(&"keepable".to_string()));

        manager.stop_and_keep("keepable").await.unwrap();

        assert!(
            manager.list().await.is_empty(),
            "active list should not contain a stopped process"
        );
        assert_eq!(
            manager.get_phase("keepable").await,
            Some(ProcessPhase::Stopped),
            "stopped process should transition to Stopped"
        );
    }

    #[tokio::test]
    async fn test_stop_and_keep_rejects_not_started() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();
        let config = auto_start_off_config("idle");

        manager.register_waiting(config, None).await;
        manager.launch_waiting("idle").await.unwrap();

        let result = manager.stop_and_keep("idle").await;
        assert!(
            result.is_err(),
            "should reject stopping a NotStarted process"
        );
    }

    #[tokio::test]
    async fn test_stop_and_keep_rejects_waiting() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();
        let config = test_config("waiter");

        manager.register_waiting(config, None).await;

        let result = manager.stop_and_keep("waiter").await;
        assert!(result.is_err(), "should reject stopping a Waiting process");
    }

    #[tokio::test]
    async fn test_stop_and_keep_rejects_unknown() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();

        let result = manager.stop_and_keep("ghost").await;
        assert!(result.is_err(), "should reject stopping an unknown process");
    }

    #[tokio::test]
    async fn test_stop_and_keep_notifies_task_system() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();

        let notify = Arc::new(Notify::new());
        manager.set_task_notify(notify.clone());

        let config = long_running_config("notifier");
        manager.start_command(&config, None).await.unwrap();

        let notified = notify.notified();
        tokio::pin!(notified);

        manager.stop_and_keep("notifier").await.unwrap();

        let completed = tokio::time::timeout(Duration::from_secs(5), notified).await;
        assert!(
            completed.is_ok(),
            "task_notify should fire after stop_and_keep"
        );
    }

    #[tokio::test]
    async fn test_stop_and_keep_then_restart() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ProcessRunner::new(temp_dir.path().to_path_buf()).unwrap();
        let config = long_running_config("restartable");

        manager.start_command(&config, None).await.unwrap();
        assert!(manager.list().await.contains(&"restartable".to_string()));

        manager.stop_and_keep("restartable").await.unwrap();
        assert_eq!(
            manager.get_phase("restartable").await,
            Some(ProcessPhase::Stopped)
        );

        manager.start_not_started("restartable").await.unwrap();
        assert!(
            manager.list().await.contains(&"restartable".to_string()),
            "process should be active again after restart"
        );

        let _ = manager.stop_all().await;
    }

    #[test]
    fn test_display_ports_merges_listen_and_ports() {
        let config = ProcessConfig {
            listen: vec![ListenSpec {
                name: "web".to_string(),
                kind: ListenKind::Tcp,
                address: Some("127.0.0.1:8080".to_string()),
                path: None,
                backlog: None,
                mode: None,
            }],
            ports: HashMap::from([("web".to_string(), 9999), ("db".to_string(), 5432)]),
            ..test_config("ports-proc")
        };

        assert_eq!(display_ports(&config), vec!["db:5432", "web:8080"]);
    }

    #[test]
    fn attach_event_serde_round_trips() {
        let event = AttachEvent::Log {
            name: "web".to_string(),
            stream: LogStream::Stderr,
            line: "boom".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AttachEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            decoded,
            AttachEvent::Log {
                name,
                stream: LogStream::Stderr,
                line,
            } if name == "web" && line == "boom"
        ));

        let initial: AttachEvent =
            serde_json::from_str(r#"{"event":"snapshot","processes":[]}"#).unwrap();
        assert!(
            matches!(initial, AttachEvent::InitialState { ref processes } if processes.is_empty())
        );
        assert_eq!(
            serde_json::to_string(&initial).unwrap(),
            r#"{"event":"snapshot","processes":[]}"#
        );
    }

    #[test]
    fn manager_residence_preserves_the_existing_mode_wire_schema() {
        assert_eq!(
            serde_json::to_value(ManagerResidence::InProcess).unwrap(),
            serde_json::json!("foreground")
        );
        assert_eq!(
            serde_json::to_value(ManagerResidence::Daemon).unwrap(),
            serde_json::json!("daemon")
        );

        let request = serde_json::to_value(ApiRequest::Residence).unwrap();
        assert_eq!(request, serde_json::json!({ "command": "mode" }));
        assert!(matches!(
            serde_json::from_value::<ApiRequest>(request).unwrap(),
            ApiRequest::Residence
        ));

        let response = serde_json::to_value(ApiResponse::Residence {
            residence: ManagerResidence::InProcess,
        })
        .unwrap();
        assert_eq!(
            response,
            serde_json::json!({ "status": "mode", "mode": "foreground" })
        );
        assert!(matches!(
            serde_json::from_value::<ApiResponse>(response).unwrap(),
            ApiResponse::Residence {
                residence: ManagerResidence::InProcess
            }
        ));
    }

    #[tokio::test]
    async fn attach_stream_surfaces_an_older_daemon_protocol_error() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let temp_dir = tempfile::tempdir().unwrap();
        let socket_path = temp_dir.path().join("old.sock");
        let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut line = String::new();
            BufReader::new(reader).read_line(&mut line).await.unwrap();
            assert!(line.contains(r#""command":"attach""#));
            let response = ApiResponse::Error {
                message: "invalid request: unknown variant `attach`".to_string(),
            };
            let mut json = serde_json::to_vec(&response).unwrap();
            json.push(b'\n');
            writer.write_all(&json).await.unwrap();
        });

        let mut stream = NativeManagerClient::attach_stream(&socket_path)
            .await
            .unwrap();
        let error = stream
            .next()
            .await
            .expect("old daemon must answer")
            .expect_err("old daemon response must surface as an attach error");
        assert!(error.to_string().contains("unknown variant"));
        server.await.unwrap();
    }
}
