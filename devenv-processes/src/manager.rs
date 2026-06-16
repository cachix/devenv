use async_trait::async_trait;
use devenv_activity::{Activity, ProcessStatus, activity};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use nix::sys::signal::{self, Signal as NixSignal};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};

/// Commands that can be sent to control processes
#[derive(Debug, Clone)]
pub enum ProcessCommand {
    /// Restart a running process, or start a stopped process
    Restart(String),
    /// Stop a running process but keep it visible and restartable
    Stop(String),
    /// Tear down the whole process manager (stop every process and shut the
    /// daemon down). Sent from the TUI's attach-mode interrupt prompt; the
    /// attached client services it by issuing a `down`.
    StopManager,
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
    /// Ask the running manager how its session was started (foreground vs
    /// daemon). Answered authoritatively by the live manager, so a missing or
    /// stale on-disk marker can never misclassify it.
    Mode,
}

/// How the running native manager session was started. The manager answers
/// this over its control socket ([`ApiRequest::Mode`]), so there is a single
/// authoritative source rather than a sibling file that can go missing or
/// stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerMode {
    /// An interactive `devenv up` (or an in-process detached manager such as
    /// `devenv test`) owns the session from a live devenv process. A
    /// `devenv up -d` from another terminal must not schedule into it.
    Foreground,
    /// A detached daemon spawned by `devenv up -d` owns the session; a later
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
    pub restart_count: usize,
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
    /// How the running manager's session was started.
    Mode { mode: ManagerMode },
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

/// Event pushed by the daemon on an `ApiRequest::Attach` connection.
/// Newline-delimited JSON, one event per line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AttachEvent {
    /// Full process list, sent once when the stream opens.
    Snapshot { processes: Vec<ProcessInfo> },
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
            Self::Exited => write!(f, "exited"),
            Self::GaveUp => write!(f, "gave_up"),
        }
    }
}

impl From<crate::supervisor_state::SupervisorPhase> for ProcessPhase {
    fn from(phase: crate::supervisor_state::SupervisorPhase) -> Self {
        match phase {
            crate::supervisor_state::SupervisorPhase::Starting => Self::Starting,
            crate::supervisor_state::SupervisorPhase::Ready => Self::Ready,
            crate::supervisor_state::SupervisorPhase::Exited => Self::Exited,
            crate::supervisor_state::SupervisorPhase::GaveUp => Self::GaveUp,
        }
    }
}

/// Clear any leftover log files for a process that is being registered but not
/// yet launched, so an attach backlog/tail can't surface output written by a
/// previous manager session (log paths are deterministic and persist across
/// runs). A process that later launches truncates these again in
/// `launch_setup`; one that never launches stays empty.
fn clear_stale_logs(state_dir: &Path, name: &str) {
    let (stdout_path, stderr_path) = crate::command::log_paths(state_dir, name);
    let _ = std::fs::write(&stdout_path, "");
    let _ = std::fs::write(&stderr_path, "");
}

/// The terminal process phase to preserve when stopping: a process that had
/// already exited or given up on its own keeps that outcome after teardown
/// (run summaries and dependents still see it); a still-starting or running
/// process has no terminal phase yet.
fn terminal_phase_of(phase: crate::supervisor_state::SupervisorPhase) -> Option<ProcessPhase> {
    match ProcessPhase::from(phase) {
        p @ (ProcessPhase::Exited | ProcessPhase::GaveUp) => Some(p),
        _ => None,
    }
}

/// The pieces of a torn-down `Active` handle that [`Manager::finish_stop`] needs
/// to abort the supervisor, kill the job, and release ports.
struct StopParts {
    job: Arc<Job>,
    supervisor_task: JoinHandle<()>,
    notify_forwarder: JoinHandle<()>,
    output_readers: Option<(JoinHandle<()>, JoinHandle<()>)>,
    ports: Vec<u16>,
}

/// Tear an `Active` handle out for stopping: preserve its terminal supervisor
/// phase, drop a `Stopped` placeholder into the map under the same lock (so the
/// entry never vanishes mid-teardown), and return the pieces `finish_stop` needs.
fn take_active_for_stop(
    handle: JobHandle,
    name: &str,
    processes: &mut HashMap<String, ProcessEntry>,
    preserve_terminal: bool,
) -> StopParts {
    let ports = declared_ports(&handle.resources.config);
    // A self-exit/give-up outcome is preserved only for shutdown teardown,
    // where the run summary and dependents still want to see how the process
    // ended. An explicit user stop (`devenv processes stop`, Ctrl-X) is the
    // user's final word, so it reports a plain `Stopped` instead — otherwise a
    // process the user stopped after it had already exited would keep showing
    // `Exited` in `devenv processes list` and count as succeeded in summaries.
    let terminal_phase = if preserve_terminal {
        terminal_phase_of(handle.status_rx.borrow().phase)
    } else {
        None
    };
    let JobHandle {
        resources,
        supervisor_task,
        notify_forwarder,
        output_readers,
        ..
    } = handle;
    let ProcessResources {
        config,
        activity,
        job,
        ..
    } = resources;

    activity.set_status(ProcessStatus::Stopping);
    processes.insert(
        name.to_string(),
        ProcessEntry::Stopped {
            config,
            activity,
            terminal_phase,
        },
    );

    StopParts {
        job,
        supervisor_task,
        notify_forwarder,
        output_readers,
        ports,
    }
}

/// Collect the names of all active (supervised) processes from the map.
fn active_names(processes: &HashMap<String, ProcessEntry>) -> Vec<String> {
    processes
        .iter()
        .filter_map(|(name, entry)| match entry {
            ProcessEntry::Active(_) => Some(name.clone()),
            ProcessEntry::NotStarted { .. }
            | ProcessEntry::Stopped { .. }
            | ProcessEntry::Waiting { .. }
            | ProcessEntry::Launching { .. } => None,
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
    /// Process was explicitly stopped by the user; can be started again.
    Stopped {
        config: ProcessConfig,
        activity: Activity,
        /// Terminal supervisor phase preserved when the process had already
        /// exited or given up on its own before the explicit stop; reported
        /// instead of `Stopped` so run summaries and dependents keep the
        /// terminal outcome after teardown.
        terminal_phase: Option<ProcessPhase>,
    },
    /// Process is waiting for dependencies before starting.
    Waiting {
        config: ProcessConfig,
        activity: Activity,
    },
    /// Dependencies satisfied; launch in progress (the child may already be
    /// spawned). Settles to `Active` on success, `Stopped` on failure or when
    /// shutdown raced the launch.
    Launching {
        config: ProcessConfig,
        activity: Activity,
    },
    /// Process is running under supervision.
    Active(JobHandle),
}

impl ProcessEntry {
    /// The process configuration backing this entry, regardless of phase.
    fn config(&self) -> &ProcessConfig {
        match self {
            ProcessEntry::NotStarted { config, .. }
            | ProcessEntry::Stopped { config, .. }
            | ProcessEntry::Waiting { config, .. }
            | ProcessEntry::Launching { config, .. } => config,
            ProcessEntry::Active(handle) => &handle.resources.config,
        }
    }
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
    /// Fired on every process-map transition; internal waiters (stop_all's
    /// Launching drain) and the forwarder tasks use it alongside task_notify.
    entries_changed: Arc<Notify>,
    /// Whether this instance owns the runtime files (socket, pid file) and should
    /// clean them up on drop. Set to false for control-client instances that
    /// connect to an existing daemon — they should not delete the daemon's files.
    owns_runtime_files: bool,
    /// The owning task scheduler (devenv-tasks) servicing `ApiRequest::Start`
    /// and the `Wait` parked judgment. The manager can't drive dependency
    /// ordering itself — the graph lives in `devenv-tasks` — so it delegates.
    /// Set once, after the manager is wrapped in an `Arc`; unset on managers
    /// without an owning scheduler.
    scheduler: std::sync::OnceLock<std::sync::Weak<dyn ProcessScheduler>>,
    /// How this session was started, reported over `ApiRequest::Mode`. Set once
    /// by the CLI that owns the manager (foreground `devenv up` vs `up -d`
    /// daemon). An unset manager answers `Foreground` — the conservative
    /// default: another terminal's `up -d` will refuse to schedule into a
    /// manager that has not declared itself a daemon.
    mode: std::sync::OnceLock<ManagerMode>,
}

/// Owner-side scheduler hooks. Implemented by the task scheduler that owns
/// this manager (devenv-tasks), keeping the dependency graph out of
/// devenv-processes. Held weakly so the manager never keeps its owner alive;
/// a dead `Weak` (the owner is gone, e.g. a `devenv test`-owned manager after
/// its run) behaves like no scheduler at all.
#[async_trait]
pub trait ProcessScheduler: Send + Sync {
    /// Service `ApiRequest::Start`: bring the named processes up honouring their
    /// dependencies, classifying every requested name into a [`StartOutcome`]
    /// bucket.
    ///
    /// Called directly from the per-connection API task, so a long-running
    /// `start` blocks only that one connection — other clients, further `Start`
    /// requests, and shutdown handling proceed concurrently. The scheduler is
    /// registered before the cold start runs; a `Start` arriving mid-startup is
    /// served concurrently — names already pre-registered `Waiting` classify
    /// as `skipped`, and the launch race handling in `launch_waiting` makes
    /// the residual pre-registration race safe.
    async fn start(&self, names: Vec<String>) -> StartOutcome;

    /// Whether the named `Waiting` process is dependency-parked: all of its
    /// unsatisfied dependencies are blocked on external action (a stopped or
    /// not-started dependency, or transitively another parked `Waiting`
    /// process). Judged live against the scheduler's graph at call time, so
    /// the `Wait` settled rule never acts on stale information.
    async fn dependency_parked(&self, process_name: &str) -> bool;
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

/// Failure bound for a single attach event write: a client that stops reading
/// (full socket buffer) must not park the handler with shutdown unobserved
/// while the event queue grows behind it.
const ATTACH_WRITE_STALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Lines of backlog per log file sent when an attach stream opens.
const ATTACH_BACKLOG_LINES: usize = 50;

/// Bound on the per-connection attach event queue. A slow client applies
/// backpressure to the feed (status diffs await a free slot) and, once the
/// queue is full, log lines are dropped rather than buffered without limit, so
/// a slow-but-alive reader cannot grow daemon memory unboundedly.
const ATTACH_EVENT_CHANNEL_CAPACITY: usize = 2048;

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
    status_tx: tokio::sync::watch::Sender<crate::supervisor_state::JobStatus>,
    status_rx: tokio::sync::watch::Receiver<crate::supervisor_state::JobStatus>,
    notify_socket: Option<Arc<NotifySocket>>,
    stdout_tailer: JoinHandle<()>,
    stderr_tailer: JoinHandle<()>,
    stderr_log: PathBuf,
}

impl LaunchSetup {
    /// Tear down a launch that never settled to `Active`: abort the output
    /// tailers and stop the spawned child with the standard grace period. Used
    /// when shutdown raced the launch or the entry changed underneath it.
    async fn abort_and_stop(self) {
        self.stdout_tailer.abort();
        self.stderr_tailer.abort();
        self.job
            .stop_with_signal(Signal::Terminate, Duration::from_secs(5))
            .await;
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
            entries_changed: Arc::new(Notify::new()),
            owns_runtime_files: true,
            scheduler: std::sync::OnceLock::new(),
            mode: std::sync::OnceLock::new(),
        })
    }

    /// Declare how this manager's session was started, so it can answer
    /// `ApiRequest::Mode` authoritatively. Set once by the owning CLI; ignored
    /// if already set.
    pub fn set_mode(&self, mode: ManagerMode) {
        let _ = self.mode.set(mode);
    }

    /// This manager's declared session mode. Defaults to `Foreground` when
    /// unset (the conservative choice: do not auto-schedule into it).
    pub fn mode(&self) -> ManagerMode {
        self.mode.get().copied().unwrap_or(ManagerMode::Foreground)
    }

    /// Register the owning task scheduler that services `ApiRequest::Start` and
    /// the `Wait` parked judgment. Without it, `Start` requests are rejected and
    /// `Waiting` entries never settle a `Wait`. Can be called after the
    /// manager is wrapped in an `Arc`; ignored if already set.
    pub fn set_scheduler(&self, scheduler: std::sync::Weak<dyn ProcessScheduler>) {
        let _ = self.scheduler.set(scheduler);
    }

    /// The owning task scheduler, if one is registered and its owner is still
    /// alive. A `None` means dependency-aware launching is unavailable (no
    /// owner, or it has been dropped) and callers fall back accordingly.
    fn scheduler(&self) -> Option<Arc<dyn ProcessScheduler>> {
        self.scheduler.get().and_then(std::sync::Weak::upgrade)
    }

    /// Mark this instance as a control client that should not clean up
    /// runtime files (socket, pid file) on drop.
    pub fn set_control_client(&mut self) {
        self.owns_runtime_files = false;
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

    /// Query the current lifecycle phase of a process entry.
    pub async fn get_phase(&self, name: &str) -> Option<ProcessPhase> {
        let processes = self.processes.read().await;
        match processes.get(name) {
            Some(ProcessEntry::NotStarted { .. }) => Some(ProcessPhase::NotStarted),
            Some(ProcessEntry::Stopped { terminal_phase, .. }) => {
                Some(terminal_phase.unwrap_or(ProcessPhase::Stopped))
            }
            Some(ProcessEntry::Waiting { .. }) => Some(ProcessPhase::Waiting),
            Some(ProcessEntry::Launching { .. }) => Some(ProcessPhase::Starting),
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
        self.state_dir.join(crate::NATIVE_SOCKET_NAME)
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
        activity.set_status(ProcessStatus::Waiting);
        let name = config.name.clone();
        clear_stale_logs(&self.state_dir, &name);
        self.processes
            .write()
            .await
            .insert(name.clone(), ProcessEntry::Waiting { config, activity });
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
        if matches!(
            processes.get(&config.name),
            Some(ProcessEntry::Active(_) | ProcessEntry::Launching { .. })
        ) {
            return;
        }
        let name = config.name.clone();
        let activity = match processes.remove(&name) {
            // Reuse the existing activity so the TUI row is preserved.
            Some(
                ProcessEntry::NotStarted { activity, .. }
                | ProcessEntry::Stopped { activity, .. }
                | ProcessEntry::Waiting { activity, .. },
            ) => activity,
            // No prior entry (or an Active/Launching we just excluded): make a fresh one.
            _ => self.create_process_activity(&config, None),
        };
        activity.reset();
        activity.set_status(ProcessStatus::Waiting);
        processes.insert(name.clone(), ProcessEntry::Waiting { config, activity });
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
        match processes.remove(name) {
            Some(ProcessEntry::Waiting { config, activity }) => {
                activity.dependency_failed();
                processes.insert(
                    name.to_string(),
                    ProcessEntry::Stopped {
                        config,
                        activity,
                        terminal_phase: None,
                    },
                );
                info!("Cancelled waiting process: {}", name);
                drop(processes);
                self.notify_lifecycle();
            }
            Some(entry) => {
                processes.insert(name.to_string(), entry);
            }
            None => {}
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
            match processes.remove(name) {
                Some(ProcessEntry::Waiting { config, activity }) => {
                    if !config.start.enable {
                        activity.set_status(ProcessStatus::NotStarted);
                        info!("Registered auto start off process: {}", name);
                        processes.insert(
                            name.to_string(),
                            ProcessEntry::NotStarted { config, activity },
                        );
                        drop(processes);
                        self.notify_lifecycle();
                        return Ok(None);
                    }
                    activity.set_status(ProcessStatus::Running);
                    processes.insert(
                        name.to_string(),
                        ProcessEntry::Launching { config, activity },
                    );
                    // No await between the Launching insert and the settle
                    // spawn: the settle task always completes even if this
                    // caller is aborted mid-launch.
                    self.spawn_launch_settle(name.to_string())
                }
                Some(entry) => {
                    processes.insert(name.to_string(), entry);
                    bail!("Process {} is not in waiting state", name)
                }
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
            activity.set_status(ProcessStatus::NotStarted);
            info!("Registered auto start off process: {}", config.name);
            clear_stale_logs(&self.state_dir, &config.name);
            processes.insert(
                config.name.clone(),
                ProcessEntry::NotStarted { config, activity },
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
            activity.set_status(ProcessStatus::Running);
            processes.insert(name.clone(), ProcessEntry::Launching { config, activity });
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
        tokio::spawn(async move {
            let (config, activity_ref) = {
                let procs = processes.read().await;
                match procs.get(&name) {
                    Some(ProcessEntry::Launching { config, activity }) => {
                        (config.clone(), activity.ref_handle())
                    }
                    _ => bail!("process {} is not launching", name),
                }
            };

            let setup = Self::launch_setup(&state_dir, &config, &activity_ref).await;

            let mut procs = processes.write().await;
            match procs.remove(&name) {
                Some(ProcessEntry::Launching { config, activity }) => match setup {
                    Err(e) => {
                        activity.fail();
                        procs.insert(
                            name.clone(),
                            ProcessEntry::Stopped {
                                config,
                                activity,
                                terminal_phase: None,
                            },
                        );
                        drop(procs);
                        notify_lifecycle_parts(&entries_changed, &task_notify);
                        Err(e)
                    }
                    Ok(setup) if shutdown.is_cancelled() => {
                        // Shutdown raced the launch: keep the entry Launching
                        // while the spawned child is stopped, so the map never
                        // reports the process gone or stopped before the child
                        // is dead. Bounded by the stop grace period.
                        procs.insert(name.clone(), ProcessEntry::Launching { config, activity });
                        drop(procs);
                        setup.abort_and_stop().await;
                        let mut procs = processes.write().await;
                        match procs.remove(&name) {
                            Some(ProcessEntry::Launching { config, activity }) => {
                                activity.set_status(ProcessStatus::Stopped);
                                procs.insert(
                                    name.clone(),
                                    ProcessEntry::Stopped {
                                        config,
                                        activity,
                                        terminal_phase: None,
                                    },
                                );
                            }
                            Some(other) => {
                                procs.insert(name.clone(), other);
                            }
                            None => {}
                        }
                        drop(procs);
                        notify_lifecycle_parts(&entries_changed, &task_notify);
                        bail!("process manager is shutting down")
                    }
                    Ok(setup) => {
                        let resources = ProcessResources {
                            config,
                            job: setup.job.clone(),
                            activity,
                            notify_socket: setup.notify_socket,
                            status_tx: setup.status_tx,
                            stderr_log: setup.stderr_log,
                        };
                        let supervisor_task =
                            crate::supervisor::spawn_supervisor(&resources, shutdown.clone());
                        let notify_forwarder = spawn_notify_forwarder(
                            task_notify.clone(),
                            Arc::clone(&entries_changed),
                            setup.status_rx.clone(),
                        );
                        procs.insert(
                            name.clone(),
                            ProcessEntry::Active(JobHandle {
                                resources,
                                status_rx: setup.status_rx,
                                supervisor_task,
                                output_readers: Some((setup.stdout_tailer, setup.stderr_tailer)),
                                notify_forwarder,
                            }),
                        );
                        drop(procs);
                        notify_lifecycle_parts(&entries_changed, &task_notify);
                        info!("Command '{}' started", name);
                        Ok(setup.job)
                    }
                },
                other => {
                    // Unreachable given every other path refuses to touch a
                    // Launching entry; defensive so a spawned child can never
                    // detach from the map.
                    if let Some(entry) = other {
                        procs.insert(name.clone(), entry);
                    }
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
    ) -> Result<LaunchSetup> {
        // Create notify socket if configured via ready.notify
        let uses_notify = config.ready.as_ref().is_some_and(|r| r.notify);
        let notify_socket = if uses_notify {
            let socket = NotifySocket::new(state_dir, &config.name).await?;
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

            // Inject OTEL trace context so instrumented subprocesses join the trace.
            cmd.envs(devenv_activity::trace_propagation_env());

            if let Some((ref fds, ref capabilities)) = process_setup {
                command_wrap.wrap(ProcessSetupWrapper::new(fds.clone(), capabilities.clone()));
            }
        });

        job.start().await;

        // Spawn file tailers to emit output to activity
        let stderr_log = proc_cmd.stderr_log.clone();
        let stdout_tailer =
            crate::log_tailer::spawn_file_tailer(proc_cmd.stdout_log, activity.clone(), false);
        let stderr_tailer =
            crate::log_tailer::spawn_file_tailer(proc_cmd.stderr_log, activity.clone(), true);

        // Create status channel for supervisor state observation
        let initial_status = crate::supervisor_state::JobStatus {
            phase: crate::supervisor_state::SupervisorPhase::Starting,
            restart_count: 0,
        };
        let (status_tx, status_rx) = tokio::sync::watch::channel(initial_status);

        // If no readiness mechanism is configured, mark the process immediately
        // ready. Uses the same predicate the supervisor relies on
        // (`has_readiness_probe`, which counts only TCP listens), so a
        // unix-domain-only `listen` — which the supervisor never probes — is
        // correctly treated as having no readiness mechanism instead of waiting
        // forever for a probe that is never spawned.
        if !config.has_readiness_probe() {
            let _ = status_tx.send(crate::supervisor_state::JobStatus {
                phase: crate::supervisor_state::SupervisorPhase::Ready,
                restart_count: 0,
            });
        }

        Ok(LaunchSetup {
            job,
            status_tx,
            status_rx,
            notify_socket,
            stdout_tailer,
            stderr_tailer,
            stderr_log,
        })
    }

    /// Shared teardown for [`Self::stop`] and [`Self::stop_and_keep`] once the
    /// `Active` handle has been extracted and a `Stopped` placeholder inserted
    /// under the write lock: abort the supervisor, forwarder, and output
    /// readers, signal the child with the grace period, wait for declared ports
    /// to be released, then mark the activity `Stopped`.
    async fn finish_stop(
        &self,
        name: &str,
        job: Arc<Job>,
        supervisor_task: JoinHandle<()>,
        notify_forwarder: JoinHandle<()>,
        output_readers: Option<(JoinHandle<()>, JoinHandle<()>)>,
        ports: Vec<u16>,
    ) {
        let grace_period = Duration::from_secs(5);

        // Abort the supervisor task first to prevent restarts
        supervisor_task.abort();
        notify_forwarder.abort();

        // Abort output reader tasks
        if let Some((stdout_reader, stderr_reader)) = output_readers {
            stdout_reader.abort();
            stderr_reader.abort();
        }

        // Send terminate signal with grace period
        job.stop_with_signal(Signal::Terminate, grace_period).await;

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

        // Update the TUI activity to Stopped only if the entry is still the
        // Stopped placeholder inserted by the caller; a concurrent re-arm may
        // have already transitioned it onward.
        {
            let processes = self.processes.read().await;
            if let Some(ProcessEntry::Stopped { activity, .. }) = processes.get(name) {
                activity.set_status(ProcessStatus::Stopped);
            }
        }

        self.notify_lifecycle();
        info!("Process {} stopped", name);
    }

    /// Stop a process by name. An explicit stop reports a plain `Stopped`
    /// (the user's final word), even if the process had already exited.
    pub async fn stop(&self, name: &str) -> Result<()> {
        self.stop_inner(name, false).await
    }

    /// Stop a process by name, optionally preserving a terminal supervisor
    /// phase (`Exited`/`GaveUp`) into the `Stopped` entry. Only shutdown
    /// teardown (`stop_all`) preserves it; explicit stops do not.
    async fn stop_inner(&self, name: &str, preserve_terminal: bool) -> Result<()> {
        // Extract the handle and immediately insert a Stopped entry so the process
        // stays visible in the map during teardown. Without this, concurrent API
        // queries (list, status) would see "not found" during the teardown window.
        let parts = {
            let mut processes = self.processes.write().await;

            match processes.remove(name) {
                Some(ProcessEntry::Active(handle)) => {
                    take_active_for_stop(handle, name, &mut processes, preserve_terminal)
                }
                Some(
                    entry @ (ProcessEntry::NotStarted { .. }
                    | ProcessEntry::Stopped { .. }
                    | ProcessEntry::Waiting { .. }
                    | ProcessEntry::Launching { .. }),
                ) => {
                    let state = match &entry {
                        ProcessEntry::NotStarted { .. } => "auto start off",
                        ProcessEntry::Stopped { .. } => "already stopped",
                        ProcessEntry::Waiting { .. } => "waiting for dependencies",
                        ProcessEntry::Launching { .. } => "starting",
                        ProcessEntry::Active(_) => unreachable!(),
                    };
                    processes.insert(name.to_string(), entry);
                    bail!("Process {} is {}, cannot stop", name, state)
                }
                None => bail!("Process {} not found", name),
            }
        };

        trace!("Stopping process: {}", name);

        self.finish_stop(
            name,
            parts.job,
            parts.supervisor_task,
            parts.notify_forwarder,
            parts.output_readers,
            parts.ports,
        )
        .await;
        Ok(())
    }

    /// Stop a running process but keep its entry in the process map so the TUI
    /// continues to show it and the user can restart it with Ctrl+R.
    ///
    /// Transitions an `Active` entry to `ProcessEntry::Stopped { .. }` — a
    /// distinct variant from `NotStarted` so callers of [`Self::get_phase`]
    /// can tell apart a process the user stopped from one that never started.
    /// Errors if the process is not currently `Active`.
    pub async fn stop_and_keep(&self, name: &str) -> Result<()> {
        // Extract the handle and insert the Stopped entry under the same write
        // lock, so the entry never vanishes mid-teardown: get_phase keeps
        // answering, run_foreground's empty-map check cannot fire, and a
        // concurrent re-arm cannot launch into a missing slot and later be
        // clobbered by a trailing insert.
        // An explicit Ctrl-X stop reports a plain `Stopped` (the user's final
        // word) rather than preserving a self-exit phase, so a process the user
        // stopped after it had exited is not later mistaken for a live one.
        let parts = {
            let mut processes = self.processes.write().await;

            match processes.remove(name) {
                Some(ProcessEntry::Active(handle)) => {
                    take_active_for_stop(handle, name, &mut processes, false)
                }
                Some(
                    entry @ (ProcessEntry::NotStarted { .. }
                    | ProcessEntry::Waiting { .. }
                    | ProcessEntry::Stopped { .. }
                    | ProcessEntry::Launching { .. }),
                ) => {
                    let state = match &entry {
                        ProcessEntry::NotStarted { .. } => "not running",
                        ProcessEntry::Stopped { .. } => "already stopped",
                        ProcessEntry::Launching { .. } => "starting",
                        _ => "waiting for dependencies",
                    };
                    processes.insert(name.to_string(), entry);
                    bail!("Process {} is {}, cannot stop", name, state)
                }
                None => bail!("Process {} not found", name),
            }
        };

        trace!("Stopping process (keeping visible): {}", name);

        self.finish_stop(
            name,
            parts.job,
            parts.supervisor_task,
            parts.notify_forwarder,
            parts.output_readers,
            parts.ports,
        )
        .await;
        Ok(())
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
                        // Shutdown teardown preserves a self-exit/give-up phase
                        // so the run summary still reflects how each process
                        // ended (unlike an explicit user stop).
                        names.iter().map(|name| self.stop_inner(name, true)),
                    )
                    .await,
                ) {
                    if let Err(err) = result {
                        warn!("Failed to stop process {}: {}", name, err);
                    }
                }
                continue;
            }

            let launching = self
                .processes
                .read()
                .await
                .values()
                .any(|e| matches!(e, ProcessEntry::Launching { .. }));
            if !launching {
                break;
            }
            // A launch is settling; it transitions to Active (stopped on the
            // next iteration) or Stopped, and fires entries_changed either way.
            notified.await;
        }

        Ok(())
    }

    /// Restart a process by name
    ///
    /// This resets the restart count and activity state, respawns the supervision
    /// task if it exited (e.g., due to max restarts), and restarts the underlying job.
    pub async fn restart(&self, name: &str) -> Result<()> {
        let mut processes = self.processes.write().await;
        // Checked under the write lock so it serializes with stop_all's
        // post-cancel map reads (see launch_waiting).
        if self.shutdown.is_cancelled() {
            bail!("process manager is shutting down");
        }
        let handle = match processes.get_mut(name) {
            Some(ProcessEntry::Active(h)) => h,
            Some(ProcessEntry::NotStarted { .. }) => {
                bail!(
                    "Process {} has auto start disabled, use 'start' instead",
                    name
                )
            }
            Some(ProcessEntry::Stopped { .. }) => {
                bail!("Process {} is stopped, use 'start' instead", name)
            }
            Some(ProcessEntry::Waiting { .. }) => {
                bail!("Process {} is waiting for dependencies", name)
            }
            Some(ProcessEntry::Launching { .. }) => {
                bail!("Process {} is starting", name)
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

    /// Start a previously not-started or stopped process, reusing its existing TUI activity.
    pub async fn start_not_started(&self, name: &str) -> Result<Arc<Job>> {
        let settle = {
            let mut processes = self.processes.write().await;
            // Checked under the write lock so it serializes with stop_all's
            // post-cancel map reads (see launch_waiting).
            if self.shutdown.is_cancelled() {
                bail!("process manager is shutting down");
            }
            match processes.get(name) {
                Some(ProcessEntry::NotStarted { .. } | ProcessEntry::Stopped { .. }) => {}
                Some(_) => bail!("Process {} is already running", name),
                None => bail!("Process {} not found", name),
            }
            // Safe: we just checked the variant above.
            let (config, activity) = match processes.remove(name).unwrap() {
                ProcessEntry::NotStarted { config, activity }
                | ProcessEntry::Stopped {
                    config, activity, ..
                } => (config, activity),
                _ => unreachable!(),
            };

            // Reset the activity so it no longer shows as stopped
            activity.reset();
            activity.set_status(ProcessStatus::Running);

            info!("Starting not-started process: {}", name);
            processes.insert(
                name.to_string(),
                ProcessEntry::Launching { config, activity },
            );
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

    /// Settled rule for `ApiRequest::Wait`: respond once no process can make
    /// further startup progress on its own. A process counts as settled when
    /// it is
    /// - Active with a supervisor phase of Ready, Exited, or GaveUp,
    /// - NotStarted or Stopped (terminal until a user starts it),
    /// - Waiting and the owning scheduler judges it dependency-parked: its
    ///   dependency chain is blocked on a stopped/not-started (or transitively
    ///   parked) dependency, which only external action can unblock. Without
    ///   this, a `Wait` against e.g. `up <name>` whose dependency was stopped
    ///   would block forever.
    ///
    /// Not settled: Launching, Active still Starting, and Waiting whose
    /// dependencies are live and progressing (about to start). `Wait` remains
    /// a legitimately long-blocking request.
    async fn handle_wait(&self) -> ApiResponse {
        // Event-driven: every relevant transition fires `entries_changed`
        // (map transitions via notify_lifecycle, supervisor phase changes via
        // the per-process forwarder). The settled judgment also depends on
        // graph-owned oneshot task statuses (via the scheduler's
        // `dependency_parked`), whose completions fire only `task_notify`,
        // so register on both. Register before checking so a transition
        // between the check and the await cannot be missed.
        let task_notify = self
            .task_notify
            .clone()
            // No scheduler-side notifier: a dummy that never fires, so the
            // loop wakes via `entries_changed` alone.
            .unwrap_or_else(|| Arc::new(Notify::new()));
        loop {
            let entries_notified = self.entries_changed.notified();
            let task_notified = task_notify.notified();
            tokio::pin!(entries_notified, task_notified);
            entries_notified.as_mut().enable();
            task_notified.as_mut().enable();
            if self.wait_settled().await {
                return ApiResponse::Ready;
            }
            tokio::select! {
                _ = &mut entries_notified => {}
                _ = &mut task_notified => {}
            }
        }
    }

    /// True when every entry is settled per the `Wait` rule documented on
    /// [`Self::handle_wait`]. Public as a test/diagnostic surface.
    ///
    /// `Waiting` entries are judged live by the owning scheduler
    /// ([`ProcessScheduler::dependency_parked`]); with no scheduler registered
    /// or one already dropped (e.g. a `devenv test`-owned manager after its
    /// run), `Waiting` is never settled, preserving the historical `Wait`
    /// semantics.
    pub async fn wait_settled(&self) -> bool {
        // Snapshot Waiting names under the map read lock and drop the guard
        // before consulting the scheduler: dependency_parked re-enters
        // get_phase, and a queued writer on this write-preferring lock would
        // deadlock that second read.
        let waiting: Vec<String> = {
            let procs = self.processes.read().await;
            let mut waiting = Vec::new();
            for (name, entry) in procs.iter() {
                match entry {
                    ProcessEntry::Launching { .. } => return false,
                    ProcessEntry::Active(handle) => {
                        let phase: ProcessPhase = handle.status_rx.borrow().phase.into();
                        if !matches!(
                            phase,
                            ProcessPhase::Ready | ProcessPhase::Exited | ProcessPhase::GaveUp
                        ) {
                            return false;
                        }
                    }
                    ProcessEntry::Waiting { .. } => waiting.push(name.clone()),
                    ProcessEntry::NotStarted { .. } | ProcessEntry::Stopped { .. } => {}
                }
            }
            waiting
        };
        if waiting.is_empty() {
            return true;
        }
        let Some(scheduler) = self.scheduler() else {
            return false;
        };
        for name in waiting {
            if !scheduler.dependency_parked(&name).await {
                return false;
            }
        }
        true
    }

    /// Start the API socket server for external queries (e.g., `devenv processes wait`).
    ///
    /// Listens on `state_dir/native.sock` using newline-delimited JSON (`ApiRequest`/`ApiResponse`).
    /// Must be called after all initial processes have been registered in `jobs`.
    pub fn start_api_server(self: &Arc<Self>) -> Result<()> {
        let sock_path = self.api_socket_path();
        let _ = std::fs::remove_file(&sock_path);

        let listener = std::os::unix::net::UnixListener::bind(&sock_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to bind API socket at {}", sock_path.display()))?;
        listener.set_nonblocking(true).into_diagnostic()?;
        let listener = tokio::net::UnixListener::from_std(listener).into_diagnostic()?;
        info!("API server listening on {}", sock_path.display());

        let manager = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let manager = Arc::clone(&manager);
                        tokio::spawn(Self::handle_api_client(stream, manager));
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

    /// Build a `ProcessInfo` from a process entry.
    fn process_info(name: &str, entry: &ProcessEntry) -> ProcessInfo {
        let (phase, restart_count) = match entry {
            ProcessEntry::NotStarted { .. } => (ProcessPhase::NotStarted, 0),
            ProcessEntry::Stopped { terminal_phase, .. } => {
                (terminal_phase.unwrap_or(ProcessPhase::Stopped), 0)
            }
            ProcessEntry::Waiting { .. } => (ProcessPhase::Waiting, 0),
            ProcessEntry::Launching { .. } => (ProcessPhase::Starting, 0),
            ProcessEntry::Active(handle) => {
                let status = handle.status_rx.borrow();
                (ProcessPhase::from(status.phase), status.restart_count)
            }
        };
        ProcessInfo {
            name: name.to_string(),
            phase,
            restart_count,
            ports: display_ports(entry.config()),
        }
    }

    fn process_not_found(name: &str) -> ApiResponse {
        ApiResponse::Error {
            message: format!("process '{}' not found", name),
        }
    }

    /// Read the last N lines from a file, returning them as a single string.
    ///
    /// Reads at most 1 MB from the end of the file to avoid loading
    /// arbitrarily large log files into memory.
    fn read_tail(path: &std::path::Path, max_lines: usize) -> String {
        use std::io::{Read, Seek, SeekFrom};

        let Ok(mut file) = std::fs::File::open(path) else {
            return String::new();
        };
        let Ok(metadata) = file.metadata() else {
            return String::new();
        };

        let file_size = metadata.len();
        let read_size = file_size.min(1024 * 1024) as usize;
        let start_pos = file_size.saturating_sub(read_size as u64);

        if file.seek(SeekFrom::Start(start_pos)).is_err() {
            return String::new();
        }

        let mut bytes = Vec::with_capacity(read_size);
        if file.read_to_end(&mut bytes).is_err() {
            return String::new();
        }

        let buf = String::from_utf8_lossy(&bytes);

        // Scan backwards to find the start of the last N lines
        let mut newline_count = 0;
        let start_byte = buf
            .rmatch_indices('\n')
            .find_map(|(i, _)| {
                newline_count += 1;
                if newline_count > max_lines {
                    Some(i + 1)
                } else {
                    None
                }
            })
            .unwrap_or(0);

        buf[start_byte..].trim_end_matches('\n').to_string()
    }

    /// Handle a single API client connection.
    async fn handle_api_client(stream: tokio::net::UnixStream, manager: Arc<Self>) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        if reader.read_line(&mut line).await.is_err() {
            return;
        }

        let response = match serde_json::from_str::<ApiRequest>(&line) {
            Ok(ApiRequest::Wait) => manager.handle_wait().await,
            Ok(ApiRequest::List) => {
                let procs = manager.processes.read().await;
                let mut list: Vec<ProcessInfo> = procs
                    .iter()
                    .map(|(name, entry)| Self::process_info(name, entry))
                    .collect();
                list.sort_by(|a, b| a.name.cmp(&b.name));
                ApiResponse::ProcessList { processes: list }
            }
            Ok(ApiRequest::Status { name }) => {
                let procs = manager.processes.read().await;
                match procs.get(&name) {
                    Some(entry) => ApiResponse::ProcessDetail {
                        info: Self::process_info(&name, entry),
                    },
                    None => Self::process_not_found(&name),
                }
            }
            Ok(ApiRequest::Logs { name, lines }) => {
                let max_lines = lines.unwrap_or(100);
                let procs = manager.processes.read().await;
                if !procs.contains_key(&name) {
                    Self::process_not_found(&name)
                } else {
                    drop(procs);
                    let (stdout_path, stderr_path) =
                        crate::command::log_paths(&manager.state_dir, &name);
                    let (stdout, stderr) = tokio::task::spawn_blocking(move || {
                        let stdout = Self::read_tail(&stdout_path, max_lines);
                        let stderr = Self::read_tail(&stderr_path, max_lines);
                        (stdout, stderr)
                    })
                    .await
                    .unwrap_or_default();
                    ApiResponse::ProcessLogs { stdout, stderr }
                }
            }
            Ok(ApiRequest::Restart { name }) => {
                let procs = manager.processes.read().await;
                match procs.get(&name) {
                    Some(ProcessEntry::NotStarted { .. } | ProcessEntry::Stopped { .. }) => {
                        drop(procs);
                        // A stopped process is brought back through the
                        // scheduler so its `after`/`before` dependencies are
                        // honoured like any other launch; the dep-blind
                        // direct start remains only as a fallback for
                        // managers without a registered scheduler.
                        match manager.scheduler() {
                            Some(scheduler) => {
                                let outcome = scheduler.start(vec![name.clone()]).await;
                                if outcome.scheduled.contains(&name)
                                    || outcome.skipped.contains(&name)
                                {
                                    ApiResponse::Ok
                                } else {
                                    ApiResponse::Error {
                                        message: format!("failed to restart process '{}'", name),
                                    }
                                }
                            }
                            None => match manager.start_not_started(&name).await {
                                Ok(_) => ApiResponse::Ok,
                                Err(e) => ApiResponse::Error {
                                    message: format!("failed to restart process '{}': {}", name, e),
                                },
                            },
                        }
                    }
                    Some(_) => {
                        drop(procs);
                        match manager.restart(&name).await {
                            Ok(()) => ApiResponse::Ok,
                            Err(e) => ApiResponse::Error {
                                message: format!("failed to restart process '{}': {}", name, e),
                            },
                        }
                    }
                    None => Self::process_not_found(&name),
                }
            }
            Ok(ApiRequest::Start { names }) => match manager.scheduler() {
                Some(scheduler) => ApiResponse::Start {
                    outcome: scheduler.start(names).await,
                },
                None => ApiResponse::Error {
                    message: "this manager has no process scheduler to handle `start`".to_string(),
                },
            },
            Ok(ApiRequest::Mode) => ApiResponse::Mode {
                mode: manager.mode(),
            },
            Ok(ApiRequest::Stop { name }) => match manager.stop(&name).await {
                Ok(()) => ApiResponse::Ok,
                Err(e) => ApiResponse::Error {
                    message: format!("failed to stop process '{}': {}", name, e),
                },
            },
            Ok(ApiRequest::Ports) => {
                let procs = manager.processes.read().await;
                let mut ports = Vec::new();
                for (name, entry) in procs.iter() {
                    for (port_name, &port) in &entry.config().ports {
                        ports.push(PortInfo {
                            process_name: name.clone(),
                            port_name: port_name.clone(),
                            port,
                        });
                    }
                }
                ApiResponse::PortAllocations { ports }
            }
            Ok(ApiRequest::Attach) => {
                Self::handle_attach_client(reader, writer, manager).await;
                return;
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

    /// Serialize one attach event as a JSON line and write it.
    async fn write_attach_event(
        writer: &mut tokio::net::unix::OwnedWriteHalf,
        event: &AttachEvent,
    ) -> std::io::Result<()> {
        use tokio::io::AsyncWriteExt;

        let mut json = serde_json::to_vec(event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        json.push(b'\n');
        writer.write_all(&json).await
    }

    /// Write one event, treating manager shutdown, a write error, and a write
    /// stalled past the failure bound all as disconnect. Returns false when
    /// the connection should be torn down.
    async fn write_attach_event_bounded(
        writer: &mut tokio::net::unix::OwnedWriteHalf,
        event: &AttachEvent,
        shutdown: &CancellationToken,
    ) -> bool {
        tokio::select! {
            res = tokio::time::timeout(
                ATTACH_WRITE_STALL_TIMEOUT,
                Self::write_attach_event(writer, event),
            ) => matches!(res, Ok(Ok(()))),
            _ = shutdown.cancelled() => false,
        }
    }

    /// Serve one attach connection: snapshot, then status diffs and log tails
    /// until the client disconnects or the manager shuts down.
    async fn handle_attach_client(
        mut reader: tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>,
        mut writer: tokio::net::unix::OwnedWriteHalf,
        manager: Arc<Self>,
    ) {
        use tokio::io::AsyncReadExt;

        // Every exit path cancels the feeder and tailers.
        let conn = CancellationToken::new();
        let _guard = conn.clone().drop_guard();

        // Snapshot under a short read lock, dropped before any I/O.
        let snapshot: Vec<ProcessInfo> = {
            let procs = manager.processes.read().await;
            let mut list: Vec<ProcessInfo> = procs
                .iter()
                .map(|(name, entry)| Self::process_info(name, entry))
                .collect();
            list.sort_by(|a, b| a.name.cmp(&b.name));
            list
        };

        if !Self::write_attach_event_bounded(
            &mut writer,
            &AttachEvent::Snapshot {
                processes: snapshot.clone(),
            },
            &manager.shutdown,
        )
        .await
        {
            return;
        }

        let (tx, mut rx) = mpsc::channel::<AttachEvent>(ATTACH_EVENT_CHANNEL_CAPACITY);
        tokio::spawn(Self::attach_feed(
            Arc::clone(&manager),
            snapshot,
            tx,
            conn.clone(),
        ));

        // Writer/disconnect loop; never touches the processes map, so no
        // lock is ever held across a write.
        let mut probe = [0u8; 64];
        loop {
            tokio::select! {
                ev = rx.recv() => match ev {
                    Some(ev) => {
                        if !Self::write_attach_event_bounded(&mut writer, &ev, &manager.shutdown)
                            .await
                        {
                            break;
                        }
                    }
                    None => break,
                },
                // The client never sends after the request line; 0 or Err
                // means it disconnected.
                n = reader.read(&mut probe) => {
                    if matches!(n, Ok(0) | Err(_)) {
                        break;
                    }
                }
                _ = manager.shutdown.cancelled() => break,
            }
        }
    }

    /// Feed an attach connection: per-process log tailers (bounded backlog,
    /// then append-only) plus status diffs woken by `entries_changed`.
    async fn attach_feed(
        manager: Arc<Self>,
        snapshot: Vec<ProcessInfo>,
        tx: mpsc::Sender<AttachEvent>,
        conn: CancellationToken,
    ) {
        let mut prev: BTreeMap<String, ProcessInfo> = snapshot
            .into_iter()
            .map(|info| (info.name.clone(), info))
            .collect();
        for name in prev.keys() {
            Self::spawn_attach_tailers(&manager.state_dir, name, &tx, &conn);
        }

        loop {
            // Register before reading so a transition between the read and
            // the await cannot be missed (same idiom as stop_all).
            let notified = manager.entries_changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            let current: BTreeMap<String, ProcessInfo> = {
                let procs = manager.processes.read().await;
                procs
                    .iter()
                    .map(|(name, entry)| (name.clone(), Self::process_info(name, entry)))
                    .collect()
            };
            // Lock released above; sends never run under it. Entries are
            // never removed from the map, so there are no removal events.
            for (name, info) in &current {
                let is_new = !prev.contains_key(name);
                if prev.get(name) != Some(info)
                    && tx
                        .send(AttachEvent::Status { info: info.clone() })
                        .await
                        .is_err()
                {
                    return;
                }
                if is_new {
                    Self::spawn_attach_tailers(&manager.state_dir, name, &tx, &conn);
                }
            }
            prev = current;

            tokio::select! {
                _ = notified => {}
                _ = conn.cancelled() => return,
            }
        }
    }

    /// Spawn stdout+stderr attach tailers for one process: emit a backlog of
    /// the last complete lines, then tail strictly append-only from the
    /// recorded byte offset. `wait_for_create` covers processes that have not
    /// started yet. Tailer handles are not retained: tailers exit via `conn`
    /// or send failure.
    fn spawn_attach_tailers(
        state_dir: &Path,
        name: &str,
        tx: &mpsc::Sender<AttachEvent>,
        conn: &CancellationToken,
    ) {
        let (stdout_path, stderr_path) = crate::command::log_paths(state_dir, name);
        for (path, stream) in [
            (stdout_path, LogStream::Stdout),
            (stderr_path, LogStream::Stderr),
        ] {
            let (backlog, offset) = crate::log_tailer::read_backlog(&path, ATTACH_BACKLOG_LINES);
            for line in backlog {
                // Best-effort: drop on a full/closed queue rather than buffer
                // without bound; the connection is going away on Closed.
                let _ = tx.try_send(AttachEvent::Log {
                    name: name.to_string(),
                    stream,
                    line,
                });
            }
            let tx = tx.clone();
            let name = name.to_string();
            crate::log_tailer::spawn_tail_to(path, offset, true, conn.clone(), move |line| {
                match tx.try_send(AttachEvent::Log {
                    name: name.clone(),
                    stream,
                    line,
                }) {
                    Ok(()) => true,
                    // Queue full: drop this line but keep tailing, so a slow
                    // client loses some output instead of the daemon buffering
                    // it without bound.
                    Err(mpsc::error::TrySendError::Full(_)) => true,
                    // Consumer gone: stop the tailer.
                    Err(mpsc::error::TrySendError::Closed(_)) => false,
                }
            });
        }
    }

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

    /// Ask a running manager how its session was started. `None` means the
    /// manager could not be reached or did not answer with a mode (e.g. a
    /// daemon predating `ApiRequest::Mode`); callers treat that as `Daemon`
    /// for backward compatibility. A live foreground manager always answers
    /// `Foreground`, so it can never be misread.
    pub async fn query_manager_mode(socket_path: &Path) -> Option<ManagerMode> {
        match Self::api_request(socket_path, &ApiRequest::Mode).await {
            Ok(ApiResponse::Mode { mode }) => Some(mode),
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
                        trace!("No jobs running, exiting");
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
                let needs_fresh_start = matches!(
                    self.processes.read().await.get(&name),
                    Some(ProcessEntry::NotStarted { .. } | ProcessEntry::Stopped { .. })
                );
                if needs_fresh_start {
                    info!("Starting inactive process: {}", name);
                    // Bring a stopped/not-started process back through the
                    // scheduler so its `after`/`before` dependencies are
                    // honoured, matching the socket `Restart`/`Start` path. The
                    // dep-blind direct start remains only as a fallback for a
                    // manager with no registered scheduler.
                    match self.scheduler() {
                        Some(scheduler) => {
                            let outcome = scheduler.start(vec![name.clone()]).await;
                            if !outcome.scheduled.contains(&name)
                                && !outcome.skipped.contains(&name)
                            {
                                warn!("Failed to start process {}", name);
                            }
                        }
                        None => {
                            if let Err(e) = self.start_not_started(&name).await {
                                warn!("Failed to start process {}: {}", name, e);
                            }
                        }
                    }
                } else {
                    info!("Restarting process: {}", name);
                    if let Err(e) = self.restart(&name).await {
                        warn!("Failed to restart process {}: {}", name, e);
                    }
                }
            }
            ProcessCommand::Stop(name) => {
                info!("Stopping process: {}", name);
                if let Err(e) = self.stop_and_keep(&name).await {
                    warn!("Failed to stop process {}: {}", name, e);
                }
            }
            // Only the attach client (run_attached_foreground) services this;
            // an in-process manager's interrupt prompt offers quit instead of
            // detach/stop, so it is never sent here.
            ProcessCommand::StopManager => {
                debug!("ignoring StopManager on an in-process manager");
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
        trace!(
            "run_foreground: ENTERED, token_cancelled={}",
            cancellation_token.is_cancelled()
        );
        info!("Manager event loop started (foreground)");
        let mut saw_processes = false;

        loop {
            tokio::select! {
                biased;
                _ = cancellation_token.cancelled() => {
                    trace!("run_foreground: cancellation detected, calling stop_all");
                    info!("Shutdown requested, stopping all processes");
                    self.stop_all().await?;
                    trace!("run_foreground: stop_all completed");
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
                    let is_empty = self.processes.read().await.is_empty();
                    if !is_empty {
                        saw_processes = true;
                    }
                    if is_empty && saw_processes {
                        trace!("All processes exited, shutting down");
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
                // Global env is the baseline; per-process values win
                let mut merged_env = env.clone();
                merged_env.extend(config.env);
                config.env = merged_env;
                result.push(config);
            }
            result
        };

        // Create parent activity for grouping all processes
        let parent_activity = activity!(INFO, operation, "Running processes");
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

        // Remove PID file (may already be gone if the daemon cleaned up)
        let _ = tokio::fs::remove_file(&manager_pid_file).await;

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

        // Detach mode is handled at the CLI level via re-exec
        // (see `devenv daemon-processes`). This trait method only supports
        // foreground mode.
        if options.detach {
            bail!("Native process manager detach is handled by the CLI via re-exec");
        }

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
        // Only clean up runtime files if this instance owns them. Control-client
        // instances (used by `devenv processes down`) should not delete the
        // daemon's socket and pid file, as the daemon may still be shutting down.
        if self.owns_runtime_files {
            let _ = std::fs::remove_file(self.api_socket_path());
            let _ = std::fs::remove_file(self.manager_pid_file());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ListenKind, ListenSpec, RestartPolicy, StartConfig};
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
    async fn test_cancel_waiting_marks_stopped() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
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
    }

    #[tokio::test]
    async fn test_launch_failure_marks_stopped() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
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
    }

    #[tokio::test]
    async fn test_supervisor_transitions_fire_task_notify() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();

        let notify = Arc::new(Notify::new());
        manager.set_task_notify(notify.clone());

        manager
            .start_command(&test_config("short-lived"), None)
            .await
            .unwrap();

        // Event-driven wait: the forwarder must wake the task system on each
        // supervisor status transition until the process reaches Exited. The
        // timeout is a failure bound, never a poll interval.
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
        // Regression (#9): a process that exits on its own reaches `Exited`
        // while still `Active`. An explicit user stop (Ctrl-X /
        // `devenv processes stop`) is the user's final word, so afterwards the
        // process must report `Stopped` — not the preserved `Exited` terminal
        // phase, which masks the stop in `devenv processes list` and miscounts
        // it as succeeded in run summaries.
        let temp_dir = tempfile::tempdir().unwrap();
        let mut manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
        let notify = Arc::new(Notify::new());
        manager.set_task_notify(notify.clone());

        // `echo hello` exits immediately; RestartPolicy::Never keeps it Exited.
        manager
            .start_command(&test_config("self-exit"), None)
            .await
            .unwrap();

        // Event-driven wait until the process has exited on its own. The
        // timeout is a failure bound, never a poll interval.
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

        // Explicit stop: the user's final word.
        manager.stop_and_keep("self-exit").await.unwrap();

        assert_eq!(
            manager.get_phase("self-exit").await,
            Some(ProcessPhase::Stopped),
            "an explicit stop must report Stopped, not the preserved Exited phase"
        );

        let _ = manager.stop_all().await;
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
    async fn test_rearm_waiting_relaunches_stopped_process() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
        let config = long_running_config("relaunch-me");

        // Launch, then stop-and-keep so the entry becomes Stopped.
        manager.register_waiting(config.clone(), None).await;
        manager.launch_waiting("relaunch-me").await.unwrap();
        manager.stop_and_keep("relaunch-me").await.unwrap();
        assert_eq!(
            manager.get_phase("relaunch-me").await,
            Some(ProcessPhase::Stopped)
        );

        // Re-arm and relaunch — mirrors `Tasks::start_with_deps` bringing a
        // stopped process back up on an attaching `devenv up`.
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
    async fn test_start_processes_preserves_process_env_over_global_env() {
        let temp_dir = tempfile::tempdir().unwrap();
        let _manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();

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
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
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
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
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
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
        let config = test_config("waiter");

        manager.register_waiting(config, None).await;

        let result = manager.stop_and_keep("waiter").await;
        assert!(result.is_err(), "should reject stopping a Waiting process");
    }

    #[tokio::test]
    async fn test_stop_and_keep_rejects_unknown() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();

        let result = manager.stop_and_keep("ghost").await;
        assert!(result.is_err(), "should reject stopping an unknown process");
    }

    #[tokio::test]
    async fn test_stop_and_keep_notifies_task_system() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();

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
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
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

    #[tokio::test]
    async fn test_handle_command_stop() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
        let config = long_running_config("cmd-stop");

        manager.start_command(&config, None).await.unwrap();
        assert!(manager.list().await.contains(&"cmd-stop".to_string()));

        manager
            .handle_command(ProcessCommand::Stop("cmd-stop".to_string()))
            .await;

        assert_eq!(
            manager.get_phase("cmd-stop").await,
            Some(ProcessPhase::Stopped),
            "handle_command(Stop) should call stop_and_keep"
        );
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

        // A listen spec shadows a same-named declared port; the result is
        // sorted, so both the daemon TUI and attach views agree.
        assert_eq!(display_ports(&config), vec!["db:5432", "web:8080"]);
    }

    /// Test scheduler with a canned parked judgment and start outcome.
    struct StubScheduler {
        parked: std::sync::atomic::AtomicBool,
        outcome: StartOutcome,
    }

    #[async_trait]
    impl ProcessScheduler for StubScheduler {
        async fn start(&self, _names: Vec<String>) -> StartOutcome {
            self.outcome.clone()
        }

        async fn dependency_parked(&self, _process_name: &str) -> bool {
            self.parked.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    fn stub_scheduler(outcome: StartOutcome) -> Arc<StubScheduler> {
        Arc::new(StubScheduler {
            parked: std::sync::atomic::AtomicBool::new(false),
            outcome,
        })
    }

    #[tokio::test]
    async fn wait_settled_judges_waiting_via_scheduler() {
        use std::sync::atomic::Ordering;

        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
        let stub = stub_scheduler(StartOutcome::default());
        let scheduler: Arc<dyn ProcessScheduler> = stub.clone();
        manager.set_scheduler(Arc::downgrade(&scheduler));

        // Empty map settles trivially.
        assert!(manager.wait_settled().await);

        // Waiting with progressing dependencies: not settled.
        manager.register_waiting(test_config("waiter"), None).await;
        assert!(!manager.wait_settled().await);

        // Waiting and dependency-parked: settled.
        stub.parked.store(true, Ordering::SeqCst);
        assert!(manager.wait_settled().await);

        // A Launching entry is never settled, even with everything parked.
        let config = test_config("mid-launch");
        let activity = manager.create_process_activity(&config, None);
        manager.processes.write().await.insert(
            "mid-launch".to_string(),
            ProcessEntry::Launching { config, activity },
        );
        assert!(!manager.wait_settled().await);
        manager.processes.write().await.remove("mid-launch");

        // NotStarted and Stopped are terminal until a user starts them:
        // settled without consulting the scheduler.
        stub.parked.store(false, Ordering::SeqCst);
        manager
            .register_waiting(auto_start_off_config("idle"), None)
            .await;
        manager.launch_waiting("idle").await.unwrap(); // -> NotStarted
        manager.cancel_waiting("waiter").await; // -> Stopped
        assert!(manager.wait_settled().await);
    }

    #[tokio::test]
    async fn wait_settled_without_scheduler_treats_waiting_as_unsettled() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();

        manager.register_waiting(test_config("waiter"), None).await;
        assert!(
            !manager.wait_settled().await,
            "without a scheduler, Waiting must keep Wait blocking (historical semantics)"
        );

        manager.cancel_waiting("waiter").await; // -> Stopped
        assert!(manager.wait_settled().await);
    }

    /// Scheduler that reports every `dependency_parked` consultation over a
    /// channel, so tests can synchronize with `handle_wait`'s loop without
    /// timing.
    struct SignalingScheduler {
        parked: std::sync::atomic::AtomicBool,
        consulted: tokio::sync::mpsc::UnboundedSender<()>,
    }

    #[async_trait]
    impl ProcessScheduler for SignalingScheduler {
        async fn start(&self, _names: Vec<String>) -> StartOutcome {
            StartOutcome::default()
        }

        async fn dependency_parked(&self, _process_name: &str) -> bool {
            let _ = self.consulted.send(());
            self.parked.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    /// Regression test: a Waiting process can flip to dependency-parked when
    /// a graph-owned oneshot dependency completes, which fires only
    /// `task_notify` (no `entries_changed` transition). `handle_wait` must
    /// wake on that signal too, or `devenv processes wait` hangs forever.
    #[tokio::test]
    async fn handle_wait_wakes_on_task_notify_only() {
        use std::sync::atomic::Ordering;

        let temp_dir = tempfile::tempdir().unwrap();
        let mut manager = NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap();
        let task_notify = Arc::new(Notify::new());
        manager.set_task_notify(task_notify.clone());
        let manager = Arc::new(manager);

        let (consulted_tx, mut consulted_rx) = tokio::sync::mpsc::unbounded_channel();
        let stub = Arc::new(SignalingScheduler {
            parked: std::sync::atomic::AtomicBool::new(false),
            consulted: consulted_tx,
        });
        let scheduler: Arc<dyn ProcessScheduler> = stub.clone();
        manager.set_scheduler(Arc::downgrade(&scheduler));

        // A Waiting entry judged progressing: Wait blocks.
        manager.register_waiting(test_config("waiter"), None).await;

        let waiter = tokio::spawn({
            let manager = Arc::clone(&manager);
            async move { manager.handle_wait().await }
        });

        // Wait until the loop has consulted the scheduler once: by then it
        // has already registered on both notifiers (register-before-check),
        // so a wakeup fired after this point cannot be missed.
        consulted_rx
            .recv()
            .await
            .expect("handle_wait must consult the scheduler for a Waiting entry");

        // Simulate an oneshot dependency completing: the only signal is the
        // graph-side task_notify; the manager map does not transition.
        stub.parked.store(true, Ordering::SeqCst);
        task_notify.notify_waiters();

        // Failure bound only; the wait itself is event-driven.
        let response = tokio::time::timeout(std::time::Duration::from_secs(60), waiter)
            .await
            .expect("handle_wait must wake on task_notify, not only entries_changed")
            .unwrap();
        assert!(matches!(response, ApiResponse::Ready));
    }

    #[tokio::test]
    async fn start_request_over_socket_uses_scheduler() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap());
        let outcome = StartOutcome {
            scheduled: vec!["a".to_string()],
            skipped: vec!["b".to_string()],
            unknown: vec!["c".to_string()],
            failed: vec!["d".to_string()],
        };
        let stub = stub_scheduler(outcome.clone());
        let scheduler: Arc<dyn ProcessScheduler> = stub.clone();
        manager.set_scheduler(Arc::downgrade(&scheduler));
        manager.start_api_server().unwrap();

        let response = NativeProcessManager::api_request(
            &manager.api_socket_path(),
            &ApiRequest::Start {
                names: vec!["a".to_string()],
            },
        )
        .await
        .unwrap();

        match response {
            ApiResponse::Start { outcome: got } => assert_eq!(got, outcome),
            other => panic!("expected Start response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn start_request_without_scheduler_errors() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap());
        manager.start_api_server().unwrap();

        let response = NativeProcessManager::api_request(
            &manager.api_socket_path(),
            &ApiRequest::Start {
                names: vec!["a".to_string()],
            },
        )
        .await
        .unwrap();

        match response {
            ApiResponse::Error { message } => {
                assert!(
                    message.contains("no process scheduler"),
                    "unexpected error message: {message}"
                );
            }
            other => panic!("expected Error response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn manager_mode_round_trips_over_socket() {
        // Regression (#15): the running manager answers its own session mode
        // over the control socket, so there is one authoritative source
        // instead of a sibling file that could go missing or stale.
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap());
        manager.set_mode(ManagerMode::Daemon);
        manager.start_api_server().unwrap();
        assert_eq!(
            NativeProcessManager::query_manager_mode(&manager.api_socket_path()).await,
            Some(ManagerMode::Daemon),
            "a daemon-declared manager must report Daemon over the socket"
        );

        // An undeclared manager defaults to Foreground (fail-closed): another
        // terminal's `up -d` refuses to schedule into a manager that has not
        // declared itself a daemon, instead of the old None=Daemon fail-open.
        let temp_dir2 = tempfile::tempdir().unwrap();
        let undeclared =
            Arc::new(NativeProcessManager::new(temp_dir2.path().to_path_buf()).unwrap());
        undeclared.start_api_server().unwrap();
        assert_eq!(
            NativeProcessManager::query_manager_mode(&undeclared.api_socket_path()).await,
            Some(ManagerMode::Foreground),
            "an undeclared manager must default to Foreground"
        );
    }

    #[test]
    fn test_attach_event_serde() {
        let event = AttachEvent::Log {
            name: "proc".to_string(),
            stream: LogStream::Stderr,
            line: "boom".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"log""#), "wire format: {json}");
        assert!(json.contains(r#""stream":"stderr""#), "wire format: {json}");

        let back: AttachEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            back,
            AttachEvent::Log {
                ref name,
                stream: LogStream::Stderr,
                ref line,
            } if name == "proc" && line == "boom"
        ));
    }

    #[tokio::test]
    async fn test_attach_stream_end_to_end() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        // Failure bound only; every wait below is event-driven.
        const BOUND: Duration = Duration::from_secs(30);

        async fn next_event(
            lines: &mut tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
        ) -> AttachEvent {
            let line = tokio::time::timeout(BOUND, lines.next_line())
                .await
                .expect("timed out waiting for attach event")
                .expect("attach stream read failed")
                .expect("attach stream closed unexpectedly");
            serde_json::from_str(&line).expect("invalid attach event")
        }

        let temp_dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap());

        // No ports/ready config: the supervisor reports Ready immediately,
        // and the echo lands in the stdout log before or shortly after the
        // attach, exercising backlog or live tail respectively.
        let config = ProcessConfig {
            name: "attach-proc".to_string(),
            exec: "echo attach-line; sleep 100".to_string(),
            restart: crate::config::RestartConfig {
                on: RestartPolicy::Never,
                max: Some(0),
                window: None,
            },
            ..Default::default()
        };
        manager.start_command(&config, None).await.unwrap();
        manager.start_api_server().unwrap();

        let mut stream = tokio::net::UnixStream::connect(manager.api_socket_path())
            .await
            .unwrap();
        stream
            .write_all(b"{\"command\":\"attach\"}\n")
            .await
            .unwrap();
        // The write half stays alive: dropping it would read as a client
        // disconnect on the server.
        let (reader, _writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();

        // 1. Snapshot arrives first and contains the process.
        match next_event(&mut lines).await {
            AttachEvent::Snapshot { processes } => {
                assert!(
                    processes.iter().any(|p| p.name == "attach-proc"),
                    "snapshot must contain attach-proc: {processes:?}"
                );
            }
            other => panic!("expected snapshot first, got {other:?}"),
        }

        // 2. The echoed line arrives as a Log event (backlog or live tail).
        loop {
            match next_event(&mut lines).await {
                AttachEvent::Log {
                    name,
                    stream: LogStream::Stdout,
                    line,
                } if name == "attach-proc" && line == "attach-line" => break,
                _ => {}
            }
        }

        // 3. An appended line is tailed append-only: it arrives exactly once
        // and the backlog line is never re-emitted.
        let (stdout_log, _) = crate::command::log_paths(temp_dir.path(), "attach-proc");
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&stdout_log)
                .unwrap();
            f.write_all(b"live-line\n").unwrap();
        }
        loop {
            match next_event(&mut lines).await {
                AttachEvent::Log {
                    name,
                    stream: LogStream::Stdout,
                    line,
                } if name == "attach-proc" => {
                    if line == "live-line" {
                        break;
                    }
                    assert_ne!(line, "attach-line", "backlog line must not be re-emitted");
                }
                _ => {}
            }
        }

        // 4. A phase change is pushed as a Status diff via entries_changed.
        manager.stop_and_keep("attach-proc").await.unwrap();
        loop {
            match next_event(&mut lines).await {
                AttachEvent::Status { info }
                    if info.name == "attach-proc" && info.phase == ProcessPhase::Stopped =>
                {
                    break;
                }
                _ => {}
            }
        }

        // 5. Manager shutdown closes the stream: drain any buffered events
        // and assert EOF.
        manager.shutdown_supervisors();
        loop {
            let line = tokio::time::timeout(BOUND, lines.next_line())
                .await
                .expect("timed out waiting for stream EOF")
                .expect("attach stream read failed");
            match line {
                None => break,
                Some(line) => {
                    let _: AttachEvent =
                        serde_json::from_str(&line).expect("invalid event before EOF");
                }
            }
        }
    }
}
