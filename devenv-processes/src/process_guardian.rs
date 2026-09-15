//! Guards and recovers process scopes created by managed processes.
//!
//! A guardian outside each service scope watches the manager through a pipe.
//! On EOF it terminates the recorded scope. Recovery records include process
//! start times to prevent cleanup after PID reuse.

use std::collections::BTreeSet;
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
use crate::process_scope::{
    PreparedProcessScope, ProcessScope, StopPolicy, process_start_time, stop_process_scopes,
};

const GUARDIAN_ARG: &str = "--devenv-session-guardian";
const GUARDIAN_READY_TIMEOUT: Duration = Duration::from_secs(5);
/// Extra time for a guardian to finish after the service grace period.
const GUARDIAN_EXIT_MARGIN: Duration = Duration::from_secs(5);

/// Set once the normal entry point enables guardian spawning.
static GUARDIANS_ENABLED: AtomicBool = AtomicBool::new(false);

/// Process scopes and guardians owned by one supervised process.
#[derive(Debug, Default)]
pub(crate) struct ProcessScopeRegistry {
    inner: Mutex<RegistryInner>,
}

#[derive(Debug, Default)]
struct RegistryInner {
    scopes: BTreeSet<ProcessScope>,
    guardians: Vec<ProcessGuardian>,
}

impl ProcessScopeRegistry {
    pub(crate) fn record(&self, scope: ProcessScope) {
        self.inner.lock().unwrap().scopes.insert(scope);
    }

    fn attach(&self, guardian: ProcessGuardian) {
        self.inner.lock().unwrap().guardians.push(guardian);
    }

    /// Stop all recorded scopes and retire their guardians.
    pub(crate) async fn cleanup(&self, shutdown: &ShutdownConfig) {
        let (scopes, guardians) = {
            let mut inner = self.inner.lock().unwrap();
            (
                std::mem::take(&mut inner.scopes),
                std::mem::take(&mut inner.guardians),
            )
        };
        if !scopes.is_empty() {
            let signal = shutdown.signal;
            let grace = shutdown.grace_duration();
            match tokio::task::spawn_blocking(move || {
                stop_process_scopes(scopes, StopPolicy { signal, grace })
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!(%error, "failed to clean up scopes"),
                Err(error) => tracing::warn!(%error, "scope cleanup task failed"),
            }
        }
        futures::future::join_all(guardians.into_iter().map(ProcessGuardian::cleanup)).await;
    }
}

/// Stop the job and its remaining scope groups within one grace period.
pub(crate) async fn stop_job(
    job: &Job,
    registry: &ProcessScopeRegistry,
    shutdown: &ShutdownConfig,
) {
    #[cfg(unix)]
    {
        // Scope cleanup is the sole signal source; job.stop only settles state.
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
    registry: &ProcessScopeRegistry,
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
pub(crate) struct ProcessClaim {
    #[cfg(unix)]
    _lock: Arc<Flock<std::fs::File>>,
}

impl ProcessClaim {
    #[cfg(unix)]
    fn claim(state_dir: &Path, process_name: &str) -> Result<Self> {
        let lock_path = process_claim_lock_path(state_dir, process_name);
        let lock_parent = lock_path
            .parent()
            .ok_or_else(|| miette::miette!("claim lock path has no parent"))?;
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
pub(crate) async fn recover_and_claim_process(
    state_dir: &Path,
    process_name: &str,
) -> Result<ProcessClaim> {
    let state_dir = state_dir.to_path_buf();
    let process_name = process_name.to_string();
    tokio::task::spawn_blocking(move || {
        let claim = ProcessClaim::claim(&state_dir, &process_name)?;
        reconcile_recovery_record(&recovery_record_path(&state_dir, &process_name))?;
        Ok(claim)
    })
    .await
    .into_diagnostic()?
}

/// Registers the scope created for each supervised spawn.
#[derive(Debug)]
pub(crate) struct ProcessScopeRegistrationWrapper {
    pub(crate) state_dir: PathBuf,
    pub(crate) process_name: String,
    pub(crate) shutdown: ShutdownConfig,
    pub(crate) registry: Arc<ProcessScopeRegistry>,
    pub(crate) _claim: ProcessClaim,
    pub(crate) prepared_scope: Option<PreparedProcessScope>,
    pub(crate) spawned_scope: Option<ProcessScope>,
}

impl Clone for ProcessScopeRegistrationWrapper {
    fn clone(&self) -> Self {
        Self {
            state_dir: self.state_dir.clone(),
            process_name: self.process_name.clone(),
            shutdown: self.shutdown.clone(),
            registry: Arc::clone(&self.registry),
            _claim: self._claim.clone(),
            prepared_scope: None,
            spawned_scope: None,
        }
    }
}

impl ProcessScopeRegistrationWrapper {
    /// Register a session leader spawned by the privileged capability broker.
    pub(crate) fn register_external_session_child(
        &self,
        child: Box<dyn ChildWrapper>,
    ) -> std::io::Result<Box<dyn ChildWrapper>> {
        let pid = child
            .id()
            .ok_or_else(|| std::io::Error::other("broker child has no PID"))?;
        let pid = i32::try_from(pid)
            .map_err(|_| std::io::Error::other("broker child PID exceeds i32::MAX"))?;
        let scope = ProcessScope::unix_session(pid)?;
        if GUARDIANS_ENABLED.load(Ordering::Relaxed) {
            let (guardian, ready) = match ProcessGuardian::spawn(
                &self.state_dir,
                &self.process_name,
                &scope,
                &self.shutdown,
            ) {
                Ok(result) => result,
                Err(error) => {
                    let _ = scope.force_kill();
                    return Err(std::io::Error::other(format!(
                        "failed to start scope guardian: {error:?}"
                    )));
                }
            };
            if !guardian_became_ready(ready, GUARDIAN_READY_TIMEOUT) {
                guardian.abort();
                let _ = scope.signal(libc::SIGKILL);
                return Err(std::io::Error::other("scope guardian did not become ready"));
            }
            self.registry.attach(guardian);
        }
        self.registry.record(scope.clone());
        Ok(crate::force_exit_registry::track_child(child, scope))
    }
}

#[cfg(unix)]
impl CommandWrapper for ProcessScopeRegistrationWrapper {
    fn pre_spawn(
        &mut self,
        command: &mut tokio::process::Command,
        _core: &CommandWrap,
    ) -> std::io::Result<()> {
        self.prepared_scope = Some(PreparedProcessScope::prepare_tokio(command)?);
        Ok(())
    }

    fn post_spawn(
        &mut self,
        _command: &mut tokio::process::Command,
        child: &mut tokio::process::Child,
        _core: &CommandWrap,
    ) -> std::io::Result<()> {
        let child_pid = child
            .id()
            .ok_or_else(|| std::io::Error::other("spawned service has no PID"))?;
        let scope = self
            .prepared_scope
            .take()
            .ok_or_else(|| std::io::Error::other("scope was not prepared"))?
            .capture(child_pid)?;
        if GUARDIANS_ENABLED.load(Ordering::Relaxed) {
            let (guardian, ready) = match ProcessGuardian::spawn(
                &self.state_dir,
                &self.process_name,
                &scope,
                &self.shutdown,
            ) {
                Ok(result) => result,
                Err(error) => {
                    kill_unprotected_scope(child, &scope);
                    return Err(std::io::Error::other(format!(
                        "failed to start scope guardian for {}: {error:?}",
                        self.process_name
                    )));
                }
            };
            if !guardian_became_ready(ready, GUARDIAN_READY_TIMEOUT) {
                guardian.abort();
                kill_unprotected_scope(child, &scope);
                return Err(std::io::Error::other(format!(
                    "scope guardian for {} did not become ready",
                    self.process_name
                )));
            }
            self.registry.attach(guardian);
        }
        self.registry.record(scope.clone());
        self.spawned_scope = Some(scope);
        Ok(())
    }

    fn wrap_child(
        &mut self,
        inner: Box<dyn ChildWrapper>,
        _core: &CommandWrap,
    ) -> std::io::Result<Box<dyn ChildWrapper>> {
        let scope = self
            .spawned_scope
            .take()
            .ok_or_else(|| std::io::Error::other("spawned scope was not captured"))?;
        Ok(crate::force_exit_registry::track_child(inner, scope))
    }
}

/// Identifies a guarded scope and protects reconciliation from PID reuse.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProcessRecoveryRecord {
    name: String,
    /// Serialized identity of the guarded process set.
    // Preserve the existing record format while using precise Rust vocabulary.
    #[serde(default, rename = "tree")]
    scope: Option<ProcessScope>,
    /// Compatibility with recovery records that stored only a Unix session.
    #[serde(default)]
    session_id: Option<i32>,
    #[serde(default)]
    leader_start: Option<u64>,
    owner_pid: i32,
    owner_start: Option<u64>,
    guardian_pid: i32,
    guardian_start: Option<u64>,
    signal: i32,
    grace_ms: u64,
}

impl ProcessRecoveryRecord {
    fn grace(&self) -> Duration {
        Duration::from_millis(self.grace_ms)
    }

    fn scope(&self) -> Option<ProcessScope> {
        self.scope.clone().or_else(|| {
            self.session_id.and_then(|session_id| {
                ProcessScope::unix_session_with_start(session_id, self.leader_start).ok()
            })
        })
    }
}

/// Guardian process owned by the manager.
struct ProcessGuardian {
    recovery_record_path: PathBuf,
    write_end: Option<std::io::PipeWriter>,
    child: std::process::Child,
    grace: Duration,
}

impl std::fmt::Debug for ProcessGuardian {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessGuardian")
            .field("recovery_record_path", &self.recovery_record_path)
            .field("pid", &self.child.id())
            .finish()
    }
}

impl ProcessGuardian {
    /// Start a guardian and return its handle and readiness pipe.
    #[cfg(unix)]
    fn spawn(
        state_dir: &Path,
        process_name: &str,
        scope: &ProcessScope,
        shutdown: &ShutdownConfig,
    ) -> Result<(Self, std::process::ChildStdout)> {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};

        #[cfg(feature = "test-all")]
        if std::env::var_os("DEVENV_TASKS_TEST_FAIL_SESSION_GUARDIAN_START").is_some() {
            std::thread::sleep(Duration::from_millis(250));
            bail!("injected scope guardian startup failure");
        }

        let recovery_record_path = recovery_record_path(state_dir, process_name);
        std::fs::create_dir_all(guardian_state_dir(state_dir)).into_diagnostic()?;
        let owner_pid = std::process::id() as i32;
        let record = ProcessRecoveryRecord {
            name: process_name.to_string(),
            scope: Some(scope.clone()),
            session_id: None,
            leader_start: None,
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
            .open(recovery_record_path.with_extension("log"))
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null());

        let mut command = Command::new(std::env::current_exe().into_diagnostic()?);
        command
            .arg(GUARDIAN_ARG)
            .arg(&recovery_record_path)
            .arg(serde_json::to_string(&record).into_diagnostic()?)
            .stdin(Stdio::from(read_end))
            .stdout(Stdio::piped())
            .stderr(log);
        // Keep the guardian outside the owner's process group.
        command.process_group(0);
        let mut child = command
            .spawn()
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to start scope guardian for {process_name}"))?;

        let ready = child.stdout.take().expect("guardian stdout is piped");
        let guardian = Self {
            recovery_record_path,
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
        _scope: &ProcessScope,
        _shutdown: &ShutdownConfig,
    ) -> Result<(Self, std::process::ChildStdout)> {
        bail!("scope guardians require Unix")
    }

    /// Close the pipe, wait for the guardian, and remove its record.
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
        let _ = std::fs::remove_file(&self.recovery_record_path);
    }

    fn abort(mut self) {
        self.write_end.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.recovery_record_path);
    }
}

#[cfg(unix)]
fn kill_unprotected_scope(child: &mut tokio::process::Child, scope: &ProcessScope) {
    let _ = scope.force_kill();
    let _ = child.start_kill();
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Ok(Some(_)) | Err(_) => break,
        }
    }
}

/// Run guardian mode if this process was started as a scope guardian.
///
/// Call this before argument parsing. A guardian invocation returns an exit
/// code; a normal invocation enables guardians and returns `None`.
pub fn maybe_run_process_guardian() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(GUARDIAN_ARG)) {
        GUARDIANS_ENABLED.store(true, Ordering::Relaxed);
        return None;
    }
    let (Some(recovery_record_path), Some(record_json)) = (args.next(), args.next()) else {
        eprintln!("devenv scope guardian: usage: {GUARDIAN_ARG} <record-path> <record-json>");
        return Some(2);
    };
    match run_process_guardian(
        PathBuf::from(recovery_record_path),
        &record_json.to_string_lossy(),
    ) {
        Ok(()) => Some(0),
        Err(error) => {
            eprintln!("devenv scope guardian: {error:?}");
            Some(1)
        }
    }
}

#[cfg(unix)]
fn run_process_guardian(recovery_record_path: PathBuf, record_json: &str) -> Result<()> {
    use std::io::Write;

    let mut record: ProcessRecoveryRecord = serde_json::from_str(record_json).into_diagnostic()?;
    record.guardian_pid = std::process::id() as i32;
    record.guardian_start = process_start_time(record.guardian_pid);
    write_atomic_json(&recovery_record_path, &record)?;

    // EOF means the owner exited. SIGUSR1 means a new manager took over.
    let (signal_read, signal_write) = std::io::pipe().into_diagnostic()?;
    signal_hook::low_level::pipe::register(signal_hook::consts::signal::SIGUSR1, signal_write)
        .into_diagnostic()?;

    let mut stdout = std::io::stdout().lock();
    stdout.write_all(b"1").into_diagnostic()?;
    stdout.flush().into_diagnostic()?;
    drop(stdout);

    wait_for_cleanup_request(&signal_read);

    if let Some(scope) = record.scope().filter(ProcessScope::matches_identity) {
        let result = stop_process_scopes(
            [scope],
            StopPolicy {
                signal: record.signal,
                grace: record.grace(),
            },
        );
        let _ = std::fs::remove_file(&recovery_record_path);
        result.into_diagnostic()?;
        return Ok(());
    }
    let _ = std::fs::remove_file(&recovery_record_path);
    Ok(())
}

#[cfg(not(unix))]
fn run_process_guardian(_recovery_record_path: PathBuf, _record_json: &str) -> Result<()> {
    bail!("scope guardians require Unix")
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

/// Wait for the guardian to report that its record is on disk.
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

/// Reconcile a record left by an earlier manager.
///
/// - The owner is alive: fail; the process is running under another manager.
/// - The guardian is alive: ask it to terminate the scope and wait.
/// - Otherwise sweep the scope directly, unless its id was recycled by an
///   unrelated scope leader since the record was written.
#[cfg(unix)]
fn reconcile_recovery_record(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let record: ProcessRecoveryRecord = match std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    {
        Some(record) => record,
        None => {
            let _ = std::fs::remove_file(path);
            return Ok(());
        }
    };

    if process_matches(record.owner_pid, record.owner_start) {
        bail!(
            "process {} is already running under devenv process {}",
            record.name,
            record.owner_pid
        );
    }

    if record.guardian_start.is_some()
        && process_matches(record.guardian_pid, record.guardian_start)
    {
        signal::kill(Pid::from_raw(record.guardian_pid), NixSignal::SIGUSR1).into_diagnostic()?;
        let deadline = Instant::now() + record.grace() + GUARDIAN_EXIT_MARGIN;
        while Instant::now() < deadline {
            if !path.exists() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        bail!(
            "previous scope guardian {} for {} did not stop",
            record.guardian_pid,
            record.name
        )
    }

    if let Some(scope) = record.scope().filter(ProcessScope::matches_identity) {
        debug!(name = %record.name, leader_pid = scope.leader_pid(), "reclaiming scope left by a previous manager");
        stop_process_scopes(
            [scope],
            StopPolicy {
                signal: record.signal,
                grace: record.grace(),
            },
        )
        .into_diagnostic()?;
    }
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[cfg(not(unix))]
fn reconcile_recovery_record(_path: &Path) -> Result<()> {
    Ok(())
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

fn guardian_state_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("guardians")
}

fn recovery_record_path(state_dir: &Path, process_name: &str) -> PathBuf {
    guardian_state_dir(state_dir).join(format!("{}.json", recovery_record_key(process_name)))
}

fn process_claim_lock_path(state_dir: &Path, process_name: &str) -> PathBuf {
    guardian_state_dir(state_dir).join(format!("{}.lock", recovery_record_key(process_name)))
}

/// Stable, bounded file name derived from the process name.
fn recovery_record_key(name: &str) -> String {
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
        .ok_or_else(|| miette::miette!("record path has no parent"))?;
    std::fs::create_dir_all(parent).into_diagnostic()?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).into_diagnostic()?;
    serde_json::to_writer(&mut temp, value).into_diagnostic()?;
    temp.persist(path).into_diagnostic()?;
    Ok(())
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
        command.spawn().expect("spawn scope leader")
    }

    fn dead_pid() -> i32 {
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn true");
        let pid = child.id() as i32;
        child.wait().expect("wait true");
        pid
    }

    fn recovery_record_for(session_id: i32, dir: &Path) -> (ProcessRecoveryRecord, PathBuf) {
        let record = ProcessRecoveryRecord {
            name: "unit-test".to_string(),
            scope: None,
            session_id: Some(session_id),
            leader_start: process_start_time(session_id),
            owner_pid: dead_pid(),
            owner_start: None,
            guardian_pid: dead_pid(),
            guardian_start: None,
            signal: 15,
            grace_ms: 1_000,
        };
        (record, recovery_record_path(dir, "unit-test"))
    }

    #[test]
    fn reads_legacy_session_recovery_record() {
        let json = format!(
            r#"{{"name":"legacy","session_id":{},"leader_start":null,"owner_pid":1,"owner_start":null,"guardian_pid":2,"guardian_start":null,"signal":15,"grace_ms":1000}}"#,
            std::process::id()
        );
        let record: ProcessRecoveryRecord =
            serde_json::from_str(&json).expect("deserialize legacy record");
        let scope = record.scope().expect("restore legacy scope");
        assert_eq!(scope.leader_pid(), std::process::id() as i32);
    }

    #[test]
    fn recovery_record_keeps_existing_serialized_field_names() {
        let (record, _) = recovery_record_for(std::process::id() as i32, Path::new("unused"));
        let value = serde_json::to_value(record).expect("serialize record");
        assert!(value.get("tree").is_some());
        assert!(value.get("session_id").is_some());
        assert!(value.get("scope").is_none());
    }

    #[test]
    fn guardian_compatibility_names_and_paths_remain_stable() {
        assert_eq!(GUARDIAN_ARG, "--devenv-session-guardian");

        let state = Path::new("/state");
        assert_eq!(guardian_state_dir(state), state.join("guardians"));
        let record = recovery_record_path(state, "unit-test");
        let claim = process_claim_lock_path(state, "unit-test");
        assert_eq!(record.parent(), Some(state.join("guardians").as_path()));
        assert_eq!(record.file_stem(), claim.file_stem());
        assert_eq!(
            record.extension().and_then(|value| value.to_str()),
            Some("json")
        );
        assert_eq!(
            claim.extension().and_then(|value| value.to_str()),
            Some("lock")
        );
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
    async fn reconcile_sweeps_scope_of_dead_guardian() {
        let dir = tempfile::tempdir().unwrap();
        let mut leader = spawn_session_leader();
        let pid = pid_of(leader.as_ref());
        let (record, path) = recovery_record_for(pid, dir.path());
        write_atomic_json(&path, &record).unwrap();

        let result = tokio::task::spawn_blocking(move || reconcile_recovery_record(&path))
            .await
            .unwrap();
        let gone = exited(&mut leader, Duration::from_secs(3)).await;
        let _ = Box::into_pin(leader.kill()).await;

        result.expect("reconcile succeeds");
        assert!(gone, "scope of a dead guardian was not reclaimed");
        assert!(
            !recovery_record_path(dir.path(), "unit-test").exists(),
            "record not removed"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reconcile_leaves_recycled_session_id_alone() {
        let dir = tempfile::tempdir().unwrap();
        let mut leader = spawn_session_leader();
        let pid = pid_of(leader.as_ref());
        let (mut record, path) = recovery_record_for(pid, dir.path());
        // A different start time means the PID now belongs to someone else.
        record.leader_start = Some(1);
        write_atomic_json(&path, &record).unwrap();

        let result = tokio::task::spawn_blocking(move || reconcile_recovery_record(&path))
            .await
            .unwrap();
        let alive = !exited(&mut leader, Duration::from_millis(500)).await;
        let _ = Box::into_pin(leader.kill()).await;

        result.expect("reconcile succeeds");
        assert!(alive, "an unrelated scope leader was killed");
        assert!(
            !recovery_record_path(dir.path(), "unit-test").exists(),
            "stale record not removed"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn manager_cleanup_leaves_recycled_session_id_alone() {
        let mut leader = spawn_session_leader();
        let pid = pid_of(leader.as_ref());
        let registry = ProcessScopeRegistry::default();
        registry
            .inner
            .lock()
            .unwrap()
            .scopes
            .insert(ProcessScope::unix_session_with_start(pid, Some(1)).expect("capture scope"));

        registry.cleanup(&ShutdownConfig::default()).await;

        let alive = !exited(&mut leader, Duration::from_millis(500)).await;
        let _ = Box::into_pin(leader.kill()).await;
        assert!(alive, "manager cleanup killed a recycled scope ID");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cleanup_targets_the_known_group_without_waiting_after_exit() {
        let mut leader = spawn_session_leader();
        let pid = pid_of(leader.as_ref());
        let scope = ProcessScope::unix_session(pid).expect("capture scope");

        assert!(
            scope.is_alive(),
            "the scope leader must be visible through its scope handle"
        );

        let started = Instant::now();
        stop_process_scopes(
            [scope],
            StopPolicy {
                signal: NixSignal::SIGTERM as i32,
                grace: Duration::from_secs(3),
            },
        )
        .expect("terminate process scope");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "cleanup waited for the grace period after the process group exited"
        );
        let gone = exited(&mut leader, Duration::from_secs(3)).await;
        let _ = Box::into_pin(leader.kill()).await;
        assert!(gone, "known scope leader group was not terminated");

        let dead_pid = dead_pid();
        assert!(
            !ProcessScope::unix_session(dead_pid)
                .expect("capture scope")
                .is_alive(),
            "a scope must stop being live after its last process exits"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reconcile_refuses_a_live_owner() {
        let dir = tempfile::tempdir().unwrap();
        let mut leader = spawn_session_leader();
        let pid = pid_of(leader.as_ref());
        let (mut record, path) = recovery_record_for(pid, dir.path());
        record.owner_pid = std::process::id() as i32;
        record.owner_start = process_start_time(record.owner_pid);
        write_atomic_json(&path, &record).unwrap();

        let result = tokio::task::spawn_blocking(move || reconcile_recovery_record(&path))
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
            recovery_record_path(dir.path(), "unit-test").exists(),
            "the live owner's record was removed"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reconcile_allows_only_one_active_manager_claimant() {
        let dir = tempfile::tempdir().unwrap();
        let mut leader = spawn_session_leader();
        let pid = pid_of(leader.as_ref());
        let (record, path) = recovery_record_for(pid, dir.path());
        write_atomic_json(&path, &record).unwrap();

        // The Job keeps a clone after launch setup returns.
        let first = recover_and_claim_process(dir.path(), "unit-test")
            .await
            .expect("first manager claims the stale record");
        let retained_by_job = first.clone();
        drop(first);
        assert!(
            exited(&mut leader, Duration::from_secs(3)).await,
            "scope of a dead guardian was not reclaimed"
        );

        let second = tokio::time::timeout(
            Duration::from_millis(250),
            recover_and_claim_process(dir.path(), "unit-test"),
        )
        .await
        .expect("a second manager must not wait on an active owner")
        .expect_err("a second manager must not claim an active process");
        assert!(format!("{second:?}").contains("already managed"));

        drop(retained_by_job);
        let next = recover_and_claim_process(dir.path(), "unit-test")
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
}
