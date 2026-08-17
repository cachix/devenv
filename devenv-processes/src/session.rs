//! Tracks the Unix sessions created by managed processes.
//!
//! A service may create more process groups inside its session. Stopping only
//! the leader's group would miss them, so cleanup scans the whole session.
//!
//! # Crash guardians
//!
//! Unix does not guarantee that descendants die with their parent, especially
//! after they create private process groups. For every service spawn, supported
//! devenv executables therefore re-exec themselves as a small guardian process
//! outside the service session. The manager owns one end of a pipe and the
//! guardian blocks on the other. Normal exit, a crash, or SIGKILL closes the
//! manager end in the kernel; EOF tells the guardian to terminate every process
//! group still in the recorded session.
//!
//! The guardian also writes a lease containing the session, owner, and guardian
//! PIDs plus their start times. A later manager locks that lease before launch,
//! verifies that the PIDs were not recycled, and either asks the old guardian
//! to clean up or sweeps the abandoned session itself. This is the same general
//! sidecar/watchdog pattern used by service supervisors, adapted here because
//! devenv cannot rely on a platform-independent parent-death signal.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use miette::{IntoDiagnostic, Result, WrapErr, bail};
use serde::{Deserialize, Serialize};
use tracing::debug;
use watchexec_supervisor::job::Job;

#[cfg(not(unix))]
use watchexec_supervisor::Signal;

#[cfg(unix)]
use nix::fcntl::{Flock, FlockArg};
#[cfg(unix)]
use nix::sys::signal::{self, Signal as NixSignal};
#[cfg(unix)]
use nix::unistd::Pid;
#[cfg(unix)]
use process_wrap::tokio::{ChildWrapper, CommandWrap, CommandWrapper};

use crate::config::ShutdownConfig;

const GUARDIAN_ARG: &str = "--devenv-session-guardian";
const GUARDIAN_READY_TIMEOUT: Duration = Duration::from_secs(5);
/// Extra time for a guardian to finish after the service grace period.
const GUARDIAN_EXIT_MARGIN: Duration = Duration::from_secs(5);

/// Whether this executable handles guardian mode.
static GUARDIANS_ENABLED: AtomicBool = AtomicBool::new(false);

/// Sessions and guardians owned by one supervised process.
#[derive(Debug, Default)]
pub(crate) struct SessionRegistry {
    inner: Mutex<RegistryInner>,
}

#[derive(Debug, Default)]
struct RegistryInner {
    sessions: BTreeMap<i32, Option<u64>>,
    guardians: Vec<SessionGuardian>,
}

impl SessionRegistry {
    pub(crate) fn record(&self, session_id: i32) {
        self.inner
            .lock()
            .unwrap()
            .sessions
            .insert(session_id, process_start_time(session_id));
    }

    fn attach(&self, guardian: SessionGuardian) {
        self.inner.lock().unwrap().guardians.push(guardian);
    }

    /// Stop all recorded sessions and retire their guardians.
    pub(crate) async fn cleanup(&self, shutdown: &ShutdownConfig) {
        let (sessions, guardians) = {
            let mut inner = self.inner.lock().unwrap();
            (
                std::mem::take(&mut inner.sessions),
                std::mem::take(&mut inner.guardians),
            )
        };
        let session_ids = sessions
            .into_iter()
            .filter_map(|(session_id, leader_start)| {
                session_matches_start(session_id, leader_start).then_some(session_id)
            })
            .collect::<BTreeSet<_>>();
        if !session_ids.is_empty() {
            let signal = shutdown.signal;
            let grace = shutdown.grace_duration();
            let _ = tokio::task::spawn_blocking(move || {
                terminate_sessions(&session_ids, signal, grace);
            })
            .await;
        }
        futures::future::join_all(guardians.into_iter().map(SessionGuardian::cleanup)).await;
    }
}

/// Stop the job and its remaining session groups within one grace period.
pub(crate) async fn stop_job(job: &Job, registry: &SessionRegistry, shutdown: &ShutdownConfig) {
    #[cfg(unix)]
    {
        // The registry owns the complete session, including the leader's
        // process group, so it must be the sole graceful signal source. Once
        // every group is gone, stop the job handle to settle its task state.
        registry.cleanup(shutdown).await;
        job.stop().await;
    }
    #[cfg(not(unix))]
    {
        job.stop_with_signal(Signal::from(shutdown.signal), shutdown.grace_duration())
            .await;
        registry.cleanup(shutdown).await;
    }
}

/// Stop the job, then restart it unless manager shutdown has begun.
pub(crate) async fn restart_job(
    job: &Job,
    registry: &SessionRegistry,
    shutdown: &ShutdownConfig,
    manager_shutdown: &tokio_util::sync::CancellationToken,
    process_stop: &tokio_util::sync::CancellationToken,
) -> bool {
    stop_job(job, registry, shutdown).await;
    if manager_shutdown.is_cancelled() || process_stop.is_cancelled() {
        return false;
    }
    job.start().await;
    true
}

/// Keeps one manager responsible for a process across restarts.
#[derive(Debug, Clone)]
pub(crate) struct SessionTakeover {
    #[cfg(unix)]
    _lock: Arc<Flock<std::fs::File>>,
}

impl SessionTakeover {
    #[cfg(unix)]
    fn claim(state_dir: &Path, process_name: &str) -> Result<Self> {
        let lock_path = takeover_lock_path(state_dir, process_name);
        let lock_parent = lock_path
            .parent()
            .ok_or_else(|| miette::miette!("takeover lock path has no parent"))?;
        std::fs::create_dir_all(lock_parent).into_diagnostic()?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .into_diagnostic()?;
        let lock = Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, error)| {
            if error == nix::errno::Errno::EWOULDBLOCK {
                miette::miette!("process {process_name} is already managed")
            } else {
                miette::miette!("failed to lock {}: {error}", lock_path.display())
            }
        })?;

        Ok(Self {
            _lock: Arc::new(lock),
        })
    }

    #[cfg(not(unix))]
    fn claim(_state_dir: &Path, _process_name: &str) -> Result<Self> {
        Ok(Self {})
    }
}

/// Reclaim stale state and return a manager-lifetime ownership guard.
pub(crate) async fn reconcile_process(
    state_dir: &Path,
    process_name: &str,
) -> Result<SessionTakeover> {
    let state_dir = state_dir.to_path_buf();
    let process_name = process_name.to_string();
    tokio::task::spawn_blocking(move || {
        let takeover = SessionTakeover::claim(&state_dir, &process_name)?;
        reconcile_lease(&lease_path(&state_dir, &process_name))?;
        Ok(takeover)
    })
    .await
    .into_diagnostic()?
}

/// Process-wrap hook installed on every supervised spawn. The spawned child
/// is a session leader, so its PID is the session id.
#[derive(Debug, Clone)]
pub(crate) struct SessionRegistrationWrapper {
    pub(crate) state_dir: PathBuf,
    pub(crate) process_name: String,
    pub(crate) shutdown: ShutdownConfig,
    pub(crate) registry: Arc<SessionRegistry>,
    pub(crate) _takeover: SessionTakeover,
}

#[cfg(unix)]
impl CommandWrapper for SessionRegistrationWrapper {
    fn post_spawn(
        &mut self,
        _command: &mut tokio::process::Command,
        child: &mut tokio::process::Child,
        _core: &CommandWrap,
    ) -> std::io::Result<()> {
        let session_id = child
            .id()
            .ok_or_else(|| std::io::Error::other("spawned service has no PID"))?
            as i32;

        if GUARDIANS_ENABLED.load(Ordering::Relaxed) {
            let (guardian, ready) = match SessionGuardian::spawn(
                &self.state_dir,
                &self.process_name,
                session_id,
                &self.shutdown,
            ) {
                Ok(result) => result,
                Err(error) => {
                    kill_unprotected_session(child, session_id);
                    return Err(std::io::Error::other(format!(
                        "failed to start session guardian for {}: {error:?}",
                        self.process_name
                    )));
                }
            };
            if !guardian_became_ready(ready, GUARDIAN_READY_TIMEOUT) {
                guardian.abort();
                kill_unprotected_session(child, session_id);
                return Err(std::io::Error::other(format!(
                    "session guardian for {} did not become ready",
                    self.process_name
                )));
            }
            self.registry.attach(guardian);
        }
        self.registry.record(session_id);
        Ok(())
    }

    fn wrap_child(
        &mut self,
        inner: Box<dyn ChildWrapper>,
        _core: &CommandWrap,
    ) -> std::io::Result<Box<dyn ChildWrapper>> {
        Ok(inner)
    }
}

/// Identifies a guarded session and protects reconciliation from PID reuse.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionLease {
    name: String,
    session_id: i32,
    /// Start time of the session leader; see [`process_start_time`].
    leader_start: Option<u64>,
    owner_pid: i32,
    owner_start: Option<u64>,
    guardian_pid: i32,
    guardian_start: Option<u64>,
    signal: i32,
    grace_ms: u64,
}

impl SessionLease {
    fn grace(&self) -> Duration {
        Duration::from_millis(self.grace_ms)
    }
}

/// Guardian process owned by the manager.
struct SessionGuardian {
    lease_path: PathBuf,
    write_end: Option<std::io::PipeWriter>,
    child: std::process::Child,
    grace: Duration,
}

impl std::fmt::Debug for SessionGuardian {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionGuardian")
            .field("lease_path", &self.lease_path)
            .field("pid", &self.child.id())
            .finish()
    }
}

impl SessionGuardian {
    /// Start a guardian and return its handle and readiness pipe.
    #[cfg(unix)]
    fn spawn(
        state_dir: &Path,
        process_name: &str,
        session_id: i32,
        shutdown: &ShutdownConfig,
    ) -> Result<(Self, std::process::ChildStdout)> {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};

        #[cfg(feature = "test-all")]
        if std::env::var_os("DEVENV_TASKS_TEST_FAIL_SESSION_GUARDIAN_START").is_some() {
            std::thread::sleep(Duration::from_millis(250));
            bail!("injected session guardian startup failure");
        }

        let lease_path = lease_path(state_dir, process_name);
        std::fs::create_dir_all(guardian_dir(state_dir)).into_diagnostic()?;
        let owner_pid = std::process::id() as i32;
        let lease = SessionLease {
            name: process_name.to_string(),
            session_id,
            leader_start: process_start_time(session_id),
            owner_pid,
            owner_start: process_start_time(owner_pid),
            guardian_pid: 0,
            guardian_start: None,
            signal: shutdown.signal,
            grace_ms: shutdown.grace_duration().as_millis() as u64,
        };

        // Only the guardian inherits the read end. The write end stays here.
        let (read_end, write_end) = std::io::pipe().into_diagnostic()?;
        let log = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(lease_path.with_extension("log"))
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null());

        let mut command = Command::new(std::env::current_exe().into_diagnostic()?);
        command
            .arg(GUARDIAN_ARG)
            .arg(&lease_path)
            .arg(serde_json::to_string(&lease).into_diagnostic()?)
            .stdin(Stdio::from(read_end))
            .stdout(Stdio::piped())
            .stderr(log);
        // Keep the guardian outside the owner's process group.
        command.process_group(0);
        let mut child = command
            .spawn()
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to start session guardian for {process_name}"))?;

        let ready = child.stdout.take().expect("guardian stdout is piped");
        let guardian = Self {
            lease_path,
            write_end: Some(write_end),
            child,
            grace: shutdown.grace_duration(),
        };
        Ok((guardian, ready))
    }

    #[cfg(not(unix))]
    fn spawn(
        _state_dir: &Path,
        _process_name: &str,
        _session_id: i32,
        _shutdown: &ShutdownConfig,
    ) -> Result<(Self, std::process::ChildStdout)> {
        bail!("session guardians require Unix")
    }

    /// Close the pipe, wait for the guardian, and remove its lease.
    async fn cleanup(mut self) {
        self.write_end.take();

        let deadline = Instant::now() + self.grace + GUARDIAN_EXIT_MARGIN;
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(None) => tokio::time::sleep(Duration::from_millis(25)).await,
                Ok(Some(_)) | Err(_) => break,
            }
        }
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = std::fs::remove_file(&self.lease_path);
    }

    fn abort(mut self) {
        self.write_end.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.lease_path);
    }
}

#[cfg(unix)]
fn kill_unprotected_session(child: &mut tokio::process::Child, session_id: i32) {
    terminate_session(session_id, NixSignal::SIGKILL as i32, Duration::ZERO);
    let _ = child.start_kill();
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Ok(Some(_)) | Err(_) => break,
        }
    }
}

/// Run guardian mode if this process was started as a session guardian.
///
/// Call this before argument parsing. A guardian invocation returns an exit
/// code; a normal invocation enables guardians and returns `None`.
pub fn maybe_run_session_guardian() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(GUARDIAN_ARG)) {
        GUARDIANS_ENABLED.store(true, Ordering::Relaxed);
        return None;
    }
    let (Some(lease_path), Some(lease_json)) = (args.next(), args.next()) else {
        eprintln!("devenv session guardian: usage: {GUARDIAN_ARG} <lease-path> <lease-json>");
        return Some(2);
    };
    match run_guardian(PathBuf::from(lease_path), &lease_json.to_string_lossy()) {
        Ok(()) => Some(0),
        Err(error) => {
            eprintln!("devenv session guardian: {error:?}");
            Some(1)
        }
    }
}

#[cfg(unix)]
fn run_guardian(lease_path: PathBuf, lease_json: &str) -> Result<()> {
    use std::io::Write;

    let mut lease: SessionLease = serde_json::from_str(lease_json).into_diagnostic()?;
    lease.guardian_pid = std::process::id() as i32;
    lease.guardian_start = process_start_time(lease.guardian_pid);
    write_atomic_json(&lease_path, &lease)?;

    // EOF means the owner exited. SIGUSR1 means a new manager took over.
    let (signal_read, signal_write) = std::io::pipe().into_diagnostic()?;
    signal_hook::low_level::pipe::register(signal_hook::consts::signal::SIGUSR1, signal_write)
        .into_diagnostic()?;

    let mut stdout = std::io::stdout().lock();
    stdout.write_all(b"1").into_diagnostic()?;
    stdout.flush().into_diagnostic()?;
    drop(stdout);

    wait_for_cleanup_request(&signal_read);

    if session_matches_lease(&lease) {
        terminate_session(lease.session_id, lease.signal, lease.grace());
    }
    let _ = std::fs::remove_file(&lease_path);
    Ok(())
}

#[cfg(not(unix))]
fn run_guardian(_lease_path: PathBuf, _lease_json: &str) -> Result<()> {
    bail!("session guardians require Unix")
}

/// Block until the owner's pipe on stdin reaches EOF or SIGUSR1 arrives.
#[cfg(unix)]
fn wait_for_cleanup_request(signal_read: &std::io::PipeReader) {
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    use std::os::fd::AsFd;

    let stdin = std::io::stdin();
    let mut byte = [0_u8; 1];
    loop {
        let mut fds = [
            PollFd::new(stdin.as_fd(), PollFlags::POLLIN),
            PollFd::new(signal_read.as_fd(), PollFlags::POLLIN),
        ];
        match poll(&mut fds, PollTimeout::NONE) {
            Err(nix::errno::Errno::EINTR) => continue,
            Err(_) => return,
            Ok(_) => {}
        }
        if fds[1].any().unwrap_or(false) {
            return;
        }
        if fds[0].any().unwrap_or(false) {
            match nix::unistd::read(stdin.as_fd(), &mut byte) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
        }
    }
}

/// Wait for the guardian to report that its lease is on disk.
#[cfg(unix)]
fn guardian_became_ready(mut ready: std::process::ChildStdout, timeout: Duration) -> bool {
    use std::io::Read;

    let mut byte = [0_u8; 1];
    wait_readable(&ready, timeout) && matches!(ready.read(&mut byte), Ok(1))
}

/// Wait until `fd` is readable, at most `timeout`.
#[cfg(unix)]
fn wait_readable(fd: &impl std::os::fd::AsFd, timeout: Duration) -> bool {
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};

    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Ok(poll_timeout) = PollTimeout::try_from(remaining) else {
            return false;
        };
        let mut fds = [PollFd::new(fd.as_fd(), PollFlags::POLLIN)];
        match poll(&mut fds, poll_timeout) {
            Ok(0) => return false,
            Ok(_) => return true,
            Err(nix::errno::Errno::EINTR) if Instant::now() < deadline => continue,
            Err(_) => return false,
        }
    }
}

/// Reconcile a lease left by an earlier manager.
///
/// - The owner is alive: fail; the process is running under another manager.
/// - The guardian is alive: ask it to terminate the session and wait.
/// - Otherwise sweep the session directly, unless its id was recycled by an
///   unrelated session leader since the lease was written.
#[cfg(unix)]
fn reconcile_lease(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let lease: SessionLease = match std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    {
        Some(lease) => lease,
        None => {
            let _ = std::fs::remove_file(path);
            return Ok(());
        }
    };

    if process_matches(lease.owner_pid, lease.owner_start) {
        bail!(
            "process {} is already running under devenv process {}",
            lease.name,
            lease.owner_pid
        );
    }

    if lease.guardian_start.is_some() && process_matches(lease.guardian_pid, lease.guardian_start) {
        signal::kill(Pid::from_raw(lease.guardian_pid), NixSignal::SIGUSR1).into_diagnostic()?;
        let deadline = Instant::now() + lease.grace() + GUARDIAN_EXIT_MARGIN;
        while Instant::now() < deadline {
            if !path.exists() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        bail!(
            "previous session guardian {} for {} did not stop",
            lease.guardian_pid,
            lease.name
        )
    }

    if session_matches_lease(&lease) {
        debug!(name = %lease.name, session_id = lease.session_id, "reclaiming session left by a previous manager");
        terminate_session(lease.session_id, lease.signal, lease.grace());
    }
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[cfg(not(unix))]
fn reconcile_lease(_path: &Path) -> Result<()> {
    Ok(())
}

/// Check that the session ID was not reused after the service ended.
#[cfg(unix)]
fn session_matches_lease(lease: &SessionLease) -> bool {
    session_matches_start(lease.session_id, lease.leader_start)
}

fn session_matches_start(session_id: i32, leader_start: Option<u64>) -> bool {
    match process_start_time(session_id) {
        Some(start) => Some(start) == leader_start,
        None => true,
    }
}

/// Whether `pid` is alive and is the process that was recorded with `start`.
#[cfg(unix)]
fn process_matches(pid: i32, start: Option<u64>) -> bool {
    if pid <= 0 || signal::kill(Pid::from_raw(pid), None).is_err() {
        return false;
    }
    match (process_start_time(pid), start) {
        (Some(now), Some(recorded)) => now == recorded,
        (None, Some(_)) => false,
        (_, None) => true,
    }
}

fn guardian_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("guardians")
}

fn lease_path(state_dir: &Path, process_name: &str) -> PathBuf {
    guardian_dir(state_dir).join(format!("{}.json", lease_key(process_name)))
}

fn takeover_lock_path(state_dir: &Path, process_name: &str) -> PathBuf {
    guardian_dir(state_dir).join(format!("{}.lock", lease_key(process_name)))
}

/// Stable, bounded file name derived from the process name.
fn lease_key(name: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn write_atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| miette::miette!("lease path has no parent"))?;
    std::fs::create_dir_all(parent).into_diagnostic()?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).into_diagnostic()?;
    serde_json::to_writer(&mut temp, value).into_diagnostic()?;
    temp.persist(path).into_diagnostic()?;
    Ok(())
}

/// Send `graceful_signal` to every process group in the session, wait up to
/// `grace` for the session to empty, then SIGKILL whatever is left.
#[cfg(unix)]
pub(crate) fn terminate_session(session_id: i32, graceful_signal: i32, grace: Duration) {
    terminate_sessions(&BTreeSet::from([session_id]), graceful_signal, grace);
}

#[cfg(unix)]
fn terminate_sessions(session_ids: &BTreeSet<i32>, graceful_signal: i32, grace: Duration) {
    let graceful_signal = NixSignal::try_from(graceful_signal).unwrap_or(NixSignal::SIGTERM);
    let groups = process_groups_in_sessions(session_ids);
    if groups.is_empty() {
        return;
    }
    debug!(
        ?session_ids,
        ?groups,
        "terminating process groups left in sessions"
    );
    signal_session_groups(&groups, graceful_signal);

    let deadline = Instant::now() + grace;
    while Instant::now() < deadline {
        if process_groups_in_sessions(session_ids).is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    signal_session_groups(&process_groups_in_sessions(session_ids), NixSignal::SIGKILL);
}

#[cfg(not(unix))]
pub(crate) fn terminate_session(_session_id: i32, _graceful_signal: i32, _grace: Duration) {}

#[cfg(not(unix))]
fn terminate_sessions(_session_ids: &BTreeSet<i32>, _graceful_signal: i32, _grace: Duration) {}

#[cfg(unix)]
fn signal_session_groups(groups: &BTreeSet<i32>, signal_value: NixSignal) {
    for pgid in groups {
        let _ = signal::killpg(Pid::from_raw(*pgid), signal_value);
    }
}

#[cfg(unix)]
fn process_groups_in_sessions(session_ids: &BTreeSet<i32>) -> BTreeSet<i32> {
    session_ids
        .iter()
        .flat_map(|session_id| session_process_groups(*session_id))
        .collect()
}

/// Process group ids of every live process in `session_id`, excluding the
/// calling process.
#[cfg(target_os = "linux")]
pub(crate) fn session_process_groups(session_id: i32) -> BTreeSet<i32> {
    let mut groups = BTreeSet::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return groups;
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        if pid == std::process::id() as i32 {
            continue;
        }
        let Some(fields) = proc_stat_fields(pid) else {
            continue;
        };
        let (Some(pgid), Some(sid)) = (
            fields.get(2).and_then(|f| f.parse::<i32>().ok()),
            fields.get(3).and_then(|f| f.parse::<i32>().ok()),
        ) else {
            continue;
        };
        if sid == session_id {
            groups.insert(pgid);
        }
    }
    groups
}

/// Fields after the command name in `/proc/<pid>/stat`.
#[cfg(target_os = "linux")]
fn proc_stat_fields(pid: i32) -> Option<Vec<String>> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, after_comm) = stat.rsplit_once(") ")?;
    Some(after_comm.split_whitespace().map(str::to_string).collect())
}

/// Start time of `pid` in a platform-specific unit, stable for the lifetime
/// of the process. `None` if the process does not exist or cannot be read.
#[cfg(target_os = "linux")]
pub(crate) fn process_start_time(pid: i32) -> Option<u64> {
    proc_stat_fields(pid)?.get(19)?.parse().ok()
}

#[cfg(target_os = "macos")]
pub(crate) fn session_process_groups(session_id: i32) -> BTreeSet<i32> {
    let mut groups = BTreeSet::new();
    let session = Pid::from_raw(session_id);
    for pid in all_pids() {
        if pid == std::process::id() as i32 {
            continue;
        }
        let pid = Pid::from_raw(pid);
        if nix::unistd::getsid(Some(pid)) != Ok(session) {
            continue;
        }
        if let Ok(pgid) = nix::unistd::getpgid(Some(pid)) {
            groups.insert(pgid.as_raw());
        }
    }
    groups
}

/// Every PID on the system. `proc_listallpids` has no safe wrapper in `nix`.
#[cfg(target_os = "macos")]
fn all_pids() -> Vec<i32> {
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Vec::new();
    }
    let mut capacity = count as usize + 64;
    loop {
        let mut pids = vec![0_i32; capacity];
        let Some(buffer_size) = pids
            .len()
            .checked_mul(std::mem::size_of::<i32>())
            .and_then(|size| libc::c_int::try_from(size).ok())
        else {
            return Vec::new();
        };
        let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), buffer_size) };
        if count <= 0 {
            return Vec::new();
        }
        if (count as usize) < capacity {
            pids.truncate(count as usize);
            pids.retain(|pid| *pid > 0);
            return pids;
        }
        let Some(next_capacity) = capacity.checked_mul(2) else {
            return Vec::new();
        };
        capacity = next_capacity;
    }
}

/// Start time in microseconds since the epoch. `proc_pidinfo` has no safe
/// wrapper in `nix`.
#[cfg(target_os = "macos")]
pub(crate) fn process_start_time(pid: i32) -> Option<u64> {
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    let info = unsafe { info.assume_init() };
    Some(info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
}

/// Portable fallback: only the session leader's own process group is known.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn session_process_groups(session_id: i32) -> BTreeSet<i32> {
    BTreeSet::from([session_id])
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn process_start_time(_pid: i32) -> Option<u64> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use process_wrap::tokio::{CommandWrap, ProcessSession};

    fn spawn_session_leader() -> Box<dyn ChildWrapper> {
        let mut command = CommandWrap::with_new("bash", |command| {
            command
                .arg("-c")
                .arg("while :; do sleep 1; done")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
        });
        command.wrap(ProcessSession);
        command.spawn().expect("spawn session leader")
    }

    fn dead_pid() -> i32 {
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn true");
        let pid = child.id() as i32;
        child.wait().expect("wait true");
        pid
    }

    fn lease_for(session_id: i32, dir: &Path) -> (SessionLease, PathBuf) {
        let lease = SessionLease {
            name: "unit-test".to_string(),
            session_id,
            leader_start: process_start_time(session_id),
            owner_pid: dead_pid(),
            owner_start: None,
            guardian_pid: dead_pid(),
            guardian_start: None,
            signal: 15,
            grace_ms: 1_000,
        };
        (lease, lease_path(dir, "unit-test"))
    }

    fn pid_of(child: &dyn ChildWrapper) -> i32 {
        child.id().expect("child pid") as i32
    }

    /// Whether the child exits within `timeout`. Reaps it, so a killed
    /// leader does not linger as a zombie that still answers `kill(pid, 0)`.
    async fn exited(child: &mut Box<dyn ChildWrapper>, timeout: Duration) -> bool {
        tokio::time::timeout(timeout, child.wait()).await.is_ok()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reconcile_sweeps_session_of_dead_guardian() {
        let dir = tempfile::tempdir().unwrap();
        let mut leader = spawn_session_leader();
        let pid = pid_of(leader.as_ref());
        let (lease, path) = lease_for(pid, dir.path());
        write_atomic_json(&path, &lease).unwrap();

        let result = tokio::task::spawn_blocking(move || reconcile_lease(&path))
            .await
            .unwrap();
        let gone = exited(&mut leader, Duration::from_secs(3)).await;
        let _ = Box::into_pin(leader.kill()).await;

        result.expect("reconcile succeeds");
        assert!(gone, "session of a dead guardian was not reclaimed");
        assert!(
            !lease_path(dir.path(), "unit-test").exists(),
            "lease not removed"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reconcile_leaves_recycled_session_id_alone() {
        let dir = tempfile::tempdir().unwrap();
        let mut leader = spawn_session_leader();
        let pid = pid_of(leader.as_ref());
        let (mut lease, path) = lease_for(pid, dir.path());
        // A different start time means the PID now belongs to someone else.
        lease.leader_start = Some(1);
        write_atomic_json(&path, &lease).unwrap();

        let result = tokio::task::spawn_blocking(move || reconcile_lease(&path))
            .await
            .unwrap();
        let alive = !exited(&mut leader, Duration::from_millis(500)).await;
        let _ = Box::into_pin(leader.kill()).await;

        result.expect("reconcile succeeds");
        assert!(alive, "an unrelated session leader was killed");
        assert!(
            !lease_path(dir.path(), "unit-test").exists(),
            "stale lease not removed"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn manager_cleanup_leaves_recycled_session_id_alone() {
        let mut leader = spawn_session_leader();
        let pid = pid_of(leader.as_ref());
        let registry = SessionRegistry::default();
        registry.inner.lock().unwrap().sessions.insert(pid, Some(1));

        registry.cleanup(&ShutdownConfig::default()).await;

        let alive = !exited(&mut leader, Duration::from_millis(500)).await;
        let _ = Box::into_pin(leader.kill()).await;
        assert!(alive, "manager cleanup killed a recycled session ID");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reconcile_refuses_a_live_owner() {
        let dir = tempfile::tempdir().unwrap();
        let mut leader = spawn_session_leader();
        let pid = pid_of(leader.as_ref());
        let (mut lease, path) = lease_for(pid, dir.path());
        lease.owner_pid = std::process::id() as i32;
        lease.owner_start = process_start_time(lease.owner_pid);
        write_atomic_json(&path, &lease).unwrap();

        let result = tokio::task::spawn_blocking(move || reconcile_lease(&path))
            .await
            .unwrap();
        let alive = !exited(&mut leader, Duration::from_millis(500)).await;
        let _ = Box::into_pin(leader.kill()).await;

        let error = result.expect_err("a live owner must be refused");
        assert!(
            format!("{error:?}").contains("already running"),
            "unexpected error: {error:?}"
        );
        assert!(alive, "the running service was killed");
        assert!(
            lease_path(dir.path(), "unit-test").exists(),
            "the live owner's lease was removed"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reconcile_allows_only_one_active_manager_claimant() {
        let dir = tempfile::tempdir().unwrap();
        let mut leader = spawn_session_leader();
        let pid = pid_of(leader.as_ref());
        let (lease, path) = lease_for(pid, dir.path());
        write_atomic_json(&path, &lease).unwrap();

        // The Job keeps a clone after launch setup returns.
        let first = reconcile_process(dir.path(), "unit-test")
            .await
            .expect("first manager claims the stale lease");
        let retained_by_job = first.clone();
        drop(first);
        assert!(
            exited(&mut leader, Duration::from_secs(3)).await,
            "session of a dead guardian was not reclaimed"
        );

        let second = tokio::time::timeout(
            Duration::from_millis(250),
            reconcile_process(dir.path(), "unit-test"),
        )
        .await
        .expect("a second manager must not wait on an active owner")
        .expect_err("a second manager must not claim an active process");
        assert!(format!("{second:?}").contains("already managed"));

        drop(retained_by_job);
        let next = reconcile_process(dir.path(), "unit-test")
            .await
            .expect("a new manager claims after the first releases ownership");
        drop(next);
    }

    #[test]
    fn process_start_time_is_stable_and_distinct() {
        let me = std::process::id() as i32;
        let first = process_start_time(me);
        assert!(first.is_some(), "own start time must be readable");
        assert_eq!(first, process_start_time(me));
        assert_eq!(process_start_time(dead_pid()), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn all_pids_includes_init() {
        assert!(all_pids().contains(&1));
    }
}
