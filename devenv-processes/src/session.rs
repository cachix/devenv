//! Tracks the Unix sessions created by managed processes.
//!
//! A service may create more process groups inside its session. Stopping only
//! the leader's group would miss them, so cleanup scans the whole session.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tracing::debug;
use watchexec_supervisor::Signal;
use watchexec_supervisor::job::Job;

#[cfg(unix)]
use nix::sys::signal::{self, Signal as NixSignal};
#[cfg(unix)]
use nix::unistd::Pid;
#[cfg(unix)]
use process_wrap::tokio::{ChildWrapper, CommandWrap, CommandWrapper};

use crate::config::ShutdownConfig;

/// Every Unix session id created for one supervised process across restarts.
#[derive(Debug, Default)]
pub struct SessionRegistry {
    session_ids: Mutex<BTreeSet<i32>>,
}

impl SessionRegistry {
    pub fn record(&self, session_id: i32) {
        self.session_ids.lock().unwrap().insert(session_id);
    }

    /// Terminate every process group left in the recorded sessions, using the
    /// configured signal and grace period, then forget the sessions. Safe to
    /// call when nothing is recorded.
    pub async fn cleanup(&self, shutdown: &ShutdownConfig) {
        let session_ids = std::mem::take(&mut *self.session_ids.lock().unwrap());
        if session_ids.is_empty() {
            return;
        }
        let signal = shutdown.signal;
        let grace = shutdown.grace_duration();
        let _ = tokio::task::spawn_blocking(move || {
            for session_id in session_ids {
                terminate_session(session_id, signal, grace);
            }
        })
        .await;
    }
}

/// Stop a job with the configured signal and grace period, then reclaim
/// every process group left in the sessions it created.
pub async fn stop_job(job: &Job, registry: &SessionRegistry, shutdown: &ShutdownConfig) {
    job.stop_with_signal(Signal::from(shutdown.signal), shutdown.grace_duration())
        .await;
    registry.cleanup(shutdown).await;
}

/// Stop the job, then restart it unless manager shutdown has begun.
pub async fn restart_job(
    job: &Job,
    registry: &SessionRegistry,
    shutdown: &ShutdownConfig,
    cancellation: &tokio_util::sync::CancellationToken,
) -> bool {
    stop_job(job, registry, shutdown).await;
    if cancellation.is_cancelled() {
        return false;
    }
    job.start().await;
    true
}

/// Process-wrap hook installed on every supervised spawn. The spawned child
/// is a session leader, so its PID is the session id.
#[derive(Debug, Clone)]
pub struct SessionRegistrationWrapper {
    pub registry: Arc<SessionRegistry>,
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

/// Send `graceful_signal` to every process group in the session, wait up to
/// `grace` for the session to empty, then SIGKILL whatever is left.
#[cfg(unix)]
pub(crate) fn terminate_session(session_id: i32, graceful_signal: i32, grace: Duration) {
    let graceful_signal = NixSignal::try_from(graceful_signal).unwrap_or(NixSignal::SIGTERM);
    let groups = session_process_groups(session_id);
    if groups.is_empty() {
        return;
    }
    debug!(
        session_id,
        ?groups,
        "terminating process groups left in session"
    );
    signal_session_groups(&groups, graceful_signal);

    let deadline = Instant::now() + grace;
    while Instant::now() < deadline {
        if session_process_groups(session_id).is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    signal_session_groups(&session_process_groups(session_id), NixSignal::SIGKILL);
}

#[cfg(not(unix))]
pub(crate) fn terminate_session(_session_id: i32, _graceful_signal: i32, _grace: Duration) {}

#[cfg(unix)]
fn signal_session_groups(groups: &BTreeSet<i32>, signal_value: NixSignal) {
    for pgid in groups {
        let _ = signal::killpg(Pid::from_raw(*pgid), signal_value);
    }
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
        let Some((pgid, sid)) = proc_stat_fields(pid, 2, 3) else {
            continue;
        };
        if sid == i64::from(session_id) {
            groups.insert(pgid as i32);
        }
    }
    groups
}

/// Two numeric fields of `/proc/<pid>/stat`, indexed from zero after the
/// parenthesised command name (field 0 is the state).
#[cfg(target_os = "linux")]
pub(crate) fn proc_stat_fields(pid: i32, first: usize, second: usize) -> Option<(i64, i64)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, after_comm) = stat.rsplit_once(") ")?;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    Some((
        fields.get(first)?.parse().ok()?,
        fields.get(second)?.parse().ok()?,
    ))
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

/// Portable fallback: only the session leader's own process group is known.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn session_process_groups(session_id: i32) -> BTreeSet<i32> {
    BTreeSet::from([session_id])
}
