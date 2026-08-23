//! Process-scope lifecycle management independent of the mechanism used to implement it.
//!
//! Callers deal with two concepts:
//!
//! - [`PreparedProcessScope`] prepares a command and captures its scope after
//!   spawn.
//! - [`ProcessScope`] is the durable identity used for signalling, liveness,
//!   and cleanup. It includes the leader's start time to avoid acting on a
//!   recycled PID.
//!
//! Unix sessions are the current portable backend because descendants may
//! create their own process groups without escaping the session. Backend
//! selection is deliberately internal so stronger containment such as Linux
//! cgroups can replace it without changing callers. A descendant can still
//! escape a session by creating a nested session, so termination also snapshots
//! the live process ancestry before signalling it.
//!
//! A process that must keep the caller's controlling terminal cannot become a
//! session leader. [`ProcessScope::descendants_of_current`] covers that case
//! with weaker containment: the process and its live descendants, reached by
//! process group where the group is the scope's own and by PID where the group
//! belongs to the caller.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

#[cfg(unix)]
use nix::sys::signal::{self, Signal};
#[cfg(unix)]
use nix::unistd::Pid;

/// Pre-spawn state for creating a process scope.
#[derive(Debug)]
pub struct PreparedProcessScope {
    backend: SpawnBackend,
}

#[derive(Debug)]
enum SpawnBackend {
    UnixSession,
}

impl PreparedProcessScope {
    /// Configure a Tokio command and retain the state needed after spawning it.
    pub fn prepare_tokio(command: &mut tokio::process::Command) -> std::io::Result<Self> {
        #[cfg(not(unix))]
        {
            let _ = command;
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "process scopes are not implemented on this platform",
            ));
        }

        #[cfg(unix)]
        {
            configure_tokio_unix_session(command);
            Ok(Self {
                backend: SpawnBackend::UnixSession,
            })
        }
    }

    /// Capture the scope created for `pid` after a successful spawn.
    pub fn capture(self, pid: u32) -> std::io::Result<ProcessScope> {
        let pid = i32::try_from(pid)
            .map_err(|_| std::io::Error::other(format!("child PID {pid} exceeds i32::MAX")))?;
        match self.backend {
            SpawnBackend::UnixSession => ProcessScope::unix_session(pid),
        }
    }
}

/// What a scope signals: whole process groups, plus processes that no group
/// can stand in for.
///
/// A scope normally reaches its members through their process groups. A leader
/// that shares a process group with the caller who started it is the exception:
/// signalling that group would reach the caller too, so the leader is named on
/// its own instead.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ScopeTargets {
    groups: BTreeSet<i32>,
    processes: BTreeSet<i32>,
}

impl ScopeTargets {
    fn from_groups(groups: BTreeSet<i32>) -> Self {
        Self {
            groups,
            processes: BTreeSet::new(),
        }
    }

    /// Absorb `other` and return what it added.
    fn absorb(&mut self, other: Self) -> Self {
        let added = Self {
            groups: other.groups.difference(&self.groups).copied().collect(),
            processes: other
                .processes
                .difference(&self.processes)
                .copied()
                .collect(),
        };
        self.groups.extend(other.groups);
        self.processes.extend(other.processes);
        added
    }

    fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.processes.is_empty()
    }
}

/// A stable handle to a contained set of processes.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProcessScope {
    backend: ScopeBackend,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case")]
// The variant names are the persisted `backend` tags.
#[allow(clippy::enum_variant_names)]
enum ScopeBackend {
    UnixSession {
        leader: ProcessIdentity,
    },
    UnixProcessGroup {
        leader: ProcessIdentity,
    },
    UnixDescendants {
        leader: ProcessIdentity,
        /// Process group the leader shares with whoever started it, when it
        /// does not lead a group of its own.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shared_group: Option<i32>,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
struct ProcessIdentity {
    pid: i32,
    start_time: Option<u64>,
    /// Boot this start time was measured against, where start times are
    /// relative to boot. See [`boot_identity`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    boot_id: Option<String>,
}

impl ProcessScope {
    /// Capture an already-spawned Unix session.
    pub(crate) fn unix_session(leader_pid: i32) -> std::io::Result<Self> {
        Ok(Self {
            backend: ScopeBackend::UnixSession {
                leader: capture_process_identity(leader_pid)?,
            },
        })
    }

    /// Restore the process-group state written before durable scope records existed.
    pub(crate) fn legacy_unix_process_group(leader_pid: i32) -> std::io::Result<Self> {
        Ok(Self {
            backend: ScopeBackend::UnixProcessGroup {
                leader: capture_process_identity(leader_pid)?,
            },
        })
    }

    /// Restore a legacy session identity from an existing guardian lease.
    pub(crate) fn unix_session_with_start(
        leader_pid: i32,
        start_time: Option<u64>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            backend: ScopeBackend::UnixSession {
                leader: process_identity(leader_pid, start_time)?,
            },
        })
    }

    /// Capture the calling process and its live descendants as a scope.
    ///
    /// For a manager that runs in the foreground and keeps the caller's
    /// controlling terminal, so it cannot start its own session. Containment is
    /// weaker than a session: a descendant that starts one of its own escapes
    /// this scope once its ancestry is gone.
    ///
    /// The process group the caller handed over is recorded so that it is never
    /// signalled. Members left in it are reached by PID instead.
    pub fn descendants_of_current() -> std::io::Result<Self> {
        let pid = std::process::id() as i32;
        Ok(Self {
            backend: ScopeBackend::UnixDescendants {
                leader: capture_process_identity(pid)?,
                shared_group: current_process_group().filter(|group| *group != pid),
            },
        })
    }

    pub fn leader_pid(&self) -> i32 {
        self.leader().pid
    }

    /// Whether this scope can still be the one originally captured.
    pub fn matches_identity(&self) -> bool {
        let leader = self.leader();
        if !boot_matches(leader.boot_id.as_deref()) {
            return false;
        }
        match process_start_time(leader.pid) {
            Some(start) => Some(start) == leader.start_time,
            // A session or process-group leader may exit while the scope still
            // has members. Its numeric ID cannot be recycled while referenced
            // by that scope, so absence of the leader is not a mismatch.
            None => true,
        }
    }

    /// Whether the scope contains a non-zombie process that can do work.
    pub fn is_alive(&self) -> bool {
        self.matches_identity() && targets_alive(&self.targets())
    }

    /// Signal everything currently contained by the scope.
    ///
    /// Sessions and live descendants are enumerated on every call. Graceful
    /// termination retains those targets for the later force-kill pass.
    pub fn signal(&self, signal_number: i32) -> std::io::Result<()> {
        if !self.matches_identity() {
            return Ok(());
        }
        signal_targets(&self.targets(), signal_number)
    }

    /// Immediately kill every process currently contained by the scope.
    pub fn force_kill(&self) -> std::io::Result<()> {
        self.signal(force_kill_signal())
    }

    fn leader(&self) -> &ProcessIdentity {
        match &self.backend {
            ScopeBackend::UnixSession { leader }
            | ScopeBackend::UnixProcessGroup { leader }
            | ScopeBackend::UnixDescendants { leader, .. } => leader,
        }
    }

    fn targets(&self) -> ScopeTargets {
        self.targets_in(&process_table())
    }

    fn targets_in(&self, processes: &[ProcessTableEntry]) -> ScopeTargets {
        match &self.backend {
            ScopeBackend::UnixSession { leader } => {
                // The session ID is also its leader's initial process-group ID.
                // Preserve it when process-table enumeration races with exit.
                let mut groups = BTreeSet::from([leader.pid]);
                groups.extend(
                    processes
                        .iter()
                        .filter(|process| {
                            process.pid != std::process::id() as i32
                                && !process.zombie
                                && process.session == leader.pid
                        })
                        .map(|process| process.process_group),
                );
                // Some nested supervisors deliberately create a new session
                // for their child. It is still part of this process scope until
                // its ancestors are signalled, so capture its group while that
                // relationship is observable.
                groups.extend(descendant_process_groups(leader.pid, processes));
                ScopeTargets::from_groups(groups)
            }
            ScopeBackend::UnixProcessGroup { leader } => {
                ScopeTargets::from_groups(BTreeSet::from([leader.pid]))
            }
            ScopeBackend::UnixDescendants {
                leader,
                shared_group,
            } => {
                // A group named by the leader's PID is the leader's own: a
                // process group ID is the PID of its leader, so that group is
                // either ours or absent entirely. `shared_group` belongs to
                // whoever started the leader, so signalling it would reach the
                // caller and everything else it started. Its members are named
                // one at a time instead.
                let mut groups = BTreeSet::from([leader.pid]);
                let mut named = BTreeSet::from([leader.pid]);
                let descendants = descendants_of(leader.pid, processes);
                for process in processes {
                    if process.pid == std::process::id() as i32
                        || process.zombie
                        || !descendants.contains(&process.pid)
                    {
                        continue;
                    }
                    if Some(process.process_group) == *shared_group {
                        named.insert(process.pid);
                    } else {
                        groups.insert(process.process_group);
                    }
                }
                ScopeTargets {
                    groups,
                    processes: named,
                }
            }
        }
    }
}

fn capture_process_identity(pid: i32) -> std::io::Result<ProcessIdentity> {
    validate_pid(pid)?;
    Ok(ProcessIdentity {
        pid,
        start_time: process_start_time(pid),
        boot_id: boot_identity(),
    })
}

fn process_identity(pid: i32, start_time: Option<u64>) -> std::io::Result<ProcessIdentity> {
    validate_pid(pid)?;
    Ok(ProcessIdentity {
        pid,
        start_time,
        boot_id: boot_identity(),
    })
}

/// Identifier of the running kernel boot, where one is needed to interpret a
/// start time.
///
/// Linux counts start times in ticks since boot, so a PID and a start time
/// together do not distinguish a process from one that took the same PID after
/// a reboot. macOS records absolute start times, which are already unambiguous.
/// A scope that outlives the machine it was captured on must therefore carry
/// the boot it was measured against.
#[cfg(target_os = "linux")]
fn boot_identity() -> Option<String> {
    let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    let boot_id = boot_id.trim();
    (!boot_id.is_empty()).then(|| boot_id.to_string())
}

#[cfg(not(target_os = "linux"))]
fn boot_identity() -> Option<String> {
    None
}

/// Process group of the calling process.
#[cfg(unix)]
fn current_process_group() -> Option<i32> {
    Some(nix::unistd::getpgrp().as_raw())
}

#[cfg(not(unix))]
fn current_process_group() -> Option<i32> {
    None
}

/// Whether a recorded boot identifier still describes the running system.
fn boot_matches(recorded: Option<&str>) -> bool {
    match (recorded, boot_identity()) {
        // Recorded where start times carry no boot dependency.
        (None, _) => true,
        (Some(recorded), Some(current)) => recorded == current,
        // The boot cannot be read now but could be then, so the start time
        // can no longer be trusted to identify the process.
        (Some(_), None) => false,
    }
}

fn validate_pid(pid: i32) -> std::io::Result<()> {
    if pid <= 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("process-scope leader PID must be positive, got {pid}"),
        ));
    }
    Ok(())
}

impl<'de> Deserialize<'de> for ProcessScope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let backend = ScopeBackend::deserialize(deserializer)?;
        let scope = Self { backend };
        if scope.leader_pid() <= 0 {
            return Err(serde::de::Error::custom(
                "process-scope leader PID must be positive",
            ));
        }
        Ok(scope)
    }
}

/// Signal and grace period used to terminate process scopes.
#[derive(Clone, Copy, Debug)]
pub struct StopPolicy {
    /// Signal sent before waiting for graceful shutdown.
    pub signal: i32,
    /// Time to wait before escalating to an immediate kill.
    pub grace: Duration,
}

/// Gracefully terminate several scopes within one shared grace period.
pub fn stop_process_scopes(
    scopes: impl IntoIterator<Item = ProcessScope>,
    policy: StopPolicy,
) -> std::io::Result<()> {
    let scopes = scopes
        .into_iter()
        .filter(ProcessScope::matches_identity)
        .collect::<BTreeSet<_>>();
    if scopes.is_empty() {
        return Ok(());
    }

    debug!(count = scopes.len(), "terminating process scopes");
    let mut first_error = None;
    // Retain the initial snapshot. Once a parent is terminated, children in a
    // nested session are reparented and can no longer be rediscovered through
    // ancestry or the original session ID.
    let processes = process_table();
    let mut scopes = scopes
        .into_iter()
        .map(|scope| {
            let targets = scope.targets_in(&processes);
            (scope, targets)
        })
        .collect::<Vec<_>>();
    for (scope, targets) in &scopes {
        if let Err(error) = signal_targets(targets, policy.signal) {
            trace!(leader_pid = scope.leader_pid(), %error, "failed to signal process scope");
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }

    let deadline = Instant::now() + policy.grace;
    while Instant::now() < deadline {
        let processes = process_table();
        let mut combined = ScopeTargets::default();
        for (scope, targets) in &mut scopes {
            if scope.matches_identity() {
                let discovered = targets.absorb(scope.targets_in(&processes));
                // A supervisor may create another process group while handling
                // the initial stop request. Give that group the same graceful
                // signal immediately instead of leaving it untouched until the
                // force-kill deadline.
                if !discovered.is_empty()
                    && let Err(error) = signal_targets(&discovered, policy.signal)
                {
                    trace!(leader_pid = scope.leader_pid(), %error, "failed to signal newly discovered process group");
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
            combined.absorb(targets.clone());
        }
        // One liveness sweep for every scope: each one walks the process table.
        if !targets_alive(&combined) {
            return first_error.map_or(Ok(()), Err);
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    let processes = process_table();
    for (scope, mut targets) in scopes {
        if scope.matches_identity() {
            targets.absorb(scope.targets_in(&processes));
        }
        if let Err(error) = signal_targets(&targets, force_kill_signal()) {
            trace!(leader_pid = scope.leader_pid(), %error, "failed to kill process scope");
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

#[cfg(unix)]
fn signal_targets(targets: &ScopeTargets, signal_number: i32) -> std::io::Result<()> {
    let group_result = signal_process_groups(&targets.groups, signal_number);
    let process_result = signal_processes(&targets.processes, signal_number);
    group_result.and(process_result)
}

#[cfg(not(unix))]
fn signal_targets(_targets: &ScopeTargets, _signal_number: i32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn signal_processes(pids: &BTreeSet<i32>, signal_number: i32) -> std::io::Result<()> {
    let signal_value = Signal::try_from(signal_number).map_err(std::io::Error::other)?;
    let mut first_error = None;
    for &pid in pids {
        match signal::kill(Pid::from_raw(pid), signal_value) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
            Err(error) if first_error.is_none() => {
                first_error = Some(std::io::Error::from(error));
            }
            Err(_) => {}
        }
    }
    first_error.map_or(Ok(()), Err)
}

#[cfg(unix)]
fn signal_process_groups(groups: &BTreeSet<i32>, signal_number: i32) -> std::io::Result<()> {
    let signal_value = Signal::try_from(signal_number).map_err(std::io::Error::other)?;
    let mut first_error = None;
    for &pgid in groups {
        match signal::killpg(Pid::from_raw(pgid), signal_value) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
            Err(error) if first_error.is_none() => {
                first_error = Some(std::io::Error::from(error));
            }
            Err(_) => {}
        }
    }
    first_error.map_or(Ok(()), Err)
}

#[cfg(not(unix))]
fn signal_process_groups(_groups: &BTreeSet<i32>, _signal_number: i32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
const fn force_kill_signal() -> i32 {
    Signal::SIGKILL as i32
}

#[cfg(not(unix))]
const fn force_kill_signal() -> i32 {
    9
}

#[cfg(unix)]
fn configure_tokio_unix_session(command: &mut tokio::process::Command) {
    // Tokio does not expose setsid directly. `setsid` is async-signal-safe,
    // and the closure performs no allocation.
    unsafe {
        command.pre_exec(|| {
            nix::unistd::setsid()
                .map(|_| ())
                .map_err(std::io::Error::from)
        });
    }
}

#[cfg(not(unix))]
fn configure_tokio_unix_session(_command: &mut tokio::process::Command) {}

/// Whether anything in the scope can still respond to signals.
fn targets_alive(targets: &ScopeTargets) -> bool {
    process_groups_alive(&targets.groups) || processes_alive(&targets.processes)
}

/// Whether any of `pids` is a live, non-zombie process.
fn processes_alive(pids: &BTreeSet<i32>) -> bool {
    if pids.is_empty() {
        return false;
    }
    process_table()
        .iter()
        .any(|process| !process.zombie && pids.contains(&process.pid))
}

/// Whether a process group still contains work that can respond to signals.
///
/// `killpg(group, 0)` reports zombie-only groups as present on Linux. Waiting
/// for those groups would consume the grace period while the direct child is
/// waiting to be reaped by another task.
#[cfg(target_os = "linux")]
fn process_groups_alive(groups: &BTreeSet<i32>) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return groups.iter().any(|group| {
            !matches!(
                signal::killpg(Pid::from_raw(*group), None),
                Err(nix::errno::Errno::ESRCH)
            )
        });
    };
    entries.flatten().any(|entry| {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            return false;
        };
        let Some(fields) = proc_stat_fields(pid) else {
            return false;
        };
        fields.first().is_some_and(|state| state != "Z")
            && fields
                .get(2)
                .and_then(|field| field.parse::<i32>().ok())
                .is_some_and(|group| groups.contains(&group))
    })
}

#[cfg(all(unix, not(target_os = "linux")))]
fn process_groups_alive(groups: &BTreeSet<i32>) -> bool {
    groups
        .iter()
        .any(|group| signal::killpg(Pid::from_raw(*group), None).is_ok())
}

#[cfg(not(unix))]
fn process_groups_alive(_groups: &BTreeSet<i32>) -> bool {
    false
}

#[derive(Clone, Copy, Debug)]
struct ProcessTableEntry {
    pid: i32,
    parent_pid: i32,
    process_group: i32,
    session: i32,
    zombie: bool,
}

/// Process groups reachable from `root_pid` through the current parent scope.
///
/// Unlike session enumeration, this deliberately crosses nested `setsid()`
/// boundaries. Callers must snapshot the result before signalling ancestors,
/// because reparenting destroys this relationship.
fn descendant_process_groups(root_pid: i32, entries: &[ProcessTableEntry]) -> BTreeSet<i32> {
    process_groups_of(&descendants_of(root_pid, entries), entries)
}

fn descendants_of(root_pid: i32, entries: &[ProcessTableEntry]) -> BTreeSet<i32> {
    let mut descendants = BTreeSet::from([root_pid]);
    loop {
        let mut changed = false;
        for entry in entries {
            if descendants.contains(&entry.parent_pid) && descendants.insert(entry.pid) {
                changed = true;
            }
        }
        if !changed {
            return descendants;
        }
    }
}

fn process_groups_of(pids: &BTreeSet<i32>, entries: &[ProcessTableEntry]) -> BTreeSet<i32> {
    entries
        .iter()
        .filter(|entry| {
            entry.pid != std::process::id() as i32 && !entry.zombie && pids.contains(&entry.pid)
        })
        .map(|entry| entry.process_group)
        .collect()
}

#[cfg(target_os = "linux")]
fn process_table() -> Vec<ProcessTableEntry> {
    let mut processes = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return processes;
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let Some(fields) = proc_stat_fields(pid) else {
            continue;
        };
        let (Some(parent_pid), Some(process_group), Some(session)) = (
            fields.get(1).and_then(|field| field.parse::<i32>().ok()),
            fields.get(2).and_then(|field| field.parse::<i32>().ok()),
            fields.get(3).and_then(|field| field.parse::<i32>().ok()),
        ) else {
            continue;
        };
        processes.push(ProcessTableEntry {
            pid,
            parent_pid,
            process_group,
            session,
            zombie: fields.first().is_some_and(|state| state == "Z"),
        });
    }
    processes
}

/// Fields after the command name in `/proc/<pid>/stat`.
#[cfg(target_os = "linux")]
fn proc_stat_fields(pid: i32) -> Option<Vec<String>> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, after_comm) = stat.rsplit_once(") ")?;
    Some(after_comm.split_whitespace().map(str::to_string).collect())
}

/// Start time of `pid` in a platform-specific unit, stable for its lifetime.
#[cfg(target_os = "linux")]
pub(crate) fn process_start_time(pid: i32) -> Option<u64> {
    proc_stat_fields(pid)?.get(19)?.parse().ok()
}

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

#[cfg(target_os = "macos")]
fn process_table() -> Vec<ProcessTableEntry> {
    all_pids()
        .into_iter()
        .filter_map(|pid| {
            let info = process_bsd_info(pid)?;
            let session = nix::unistd::getsid(Some(Pid::from_raw(pid))).ok()?;
            Some(ProcessTableEntry {
                pid,
                parent_pid: i32::try_from(info.pbi_ppid).ok()?,
                process_group: i32::try_from(info.pbi_pgid).ok()?,
                session: session.as_raw(),
                zombie: info.pbi_status == libc::SZOMB,
            })
        })
        .collect()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_table() -> Vec<ProcessTableEntry> {
    Vec::new()
}

#[cfg(target_os = "macos")]
fn process_bsd_info(pid: i32) -> Option<libc::proc_bsdinfo> {
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
    (written == size).then(|| unsafe { info.assume_init() })
}

#[cfg(target_os = "macos")]
pub(crate) fn process_start_time(pid: i32) -> Option<u64> {
    let info = process_bsd_info(pid)?;
    Some(info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn process_start_time(_pid: i32) -> Option<u64> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use process_wrap::tokio::{ChildWrapper, CommandWrap, ProcessSession};

    const NESTED_SESSION_PID_FILE: &str = "DEVENV_TEST_NESTED_SESSION_PID_FILE";

    fn spawn(script: &str) -> Box<dyn ChildWrapper> {
        let mut command = CommandWrap::with_new("bash", |command| {
            command
                .arg("-c")
                .arg(script)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
        });
        command.wrap(ProcessSession);
        command.spawn().expect("spawn session leader")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn terminates_children_inside_a_session() {
        let mut child = spawn("sleep 300 & wait");
        let scope = ProcessScope::unix_session(child.id().expect("child pid") as i32)
            .expect("capture process scope");

        stop_process_scopes(
            [scope],
            StopPolicy {
                signal: Signal::SIGTERM as i32,
                grace: Duration::from_secs(2),
            },
        )
        .expect("terminate process scope");

        let status = tokio::time::timeout(Duration::from_secs(2), child.wait())
            .await
            .expect("session leader exits")
            .expect("wait succeeds");
        assert!(!status.success());
    }

    // These two ignored tests are subprocess entry points for the regression
    // below. Invoking the current test binary keeps the fixture self-contained
    // because a `setsid` executable is not uniformly available across Unix.
    #[test]
    #[ignore]
    fn nested_session_helper() {
        let pid_file = std::env::var_os(NESTED_SESSION_PID_FILE).expect("nested PID file");
        std::fs::write(pid_file, std::process::id().to_string()).expect("write nested PID");
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }

    #[test]
    #[ignore]
    fn process_scope_root_helper() {
        use std::os::unix::process::CommandExt;

        let mut command = std::process::Command::new(std::env::current_exe().expect("test binary"));
        command
            .args([
                "--ignored",
                "--exact",
                "process_scope::tests::nested_session_helper",
            ])
            .env(
                NESTED_SESSION_PID_FILE,
                std::env::var_os(NESTED_SESSION_PID_FILE).expect("nested PID file"),
            )
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        unsafe {
            command.pre_exec(|| {
                nix::unistd::setsid()
                    .map(|_| ())
                    .map_err(std::io::Error::from)
            });
        }
        let mut nested = command.spawn().expect("spawn nested session");
        let _ = nested.wait();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn terminates_descendant_that_creates_a_nested_session() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let pid_file = temp.path().join("nested.pid");
        let mut command =
            tokio::process::Command::new(std::env::current_exe().expect("test binary"));
        command
            .args([
                "--ignored",
                "--exact",
                "process_scope::tests::process_scope_root_helper",
            ])
            .env(NESTED_SESSION_PID_FILE, &pid_file)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let spawn =
            PreparedProcessScope::prepare_tokio(&mut command).expect("prepare process scope");
        let mut root = command.spawn().expect("spawn process-scope root");
        let root_pid = root.id().expect("root PID");
        let scope = spawn.capture(root_pid).expect("capture process scope");

        let deadline = Instant::now() + Duration::from_secs(5);
        let nested_pid = loop {
            if let Ok(pid) = std::fs::read_to_string(&pid_file) {
                break pid.parse::<i32>().expect("parse nested PID");
            }
            if Instant::now() >= deadline {
                scope.force_kill().expect("kill root after fixture timeout");
                let _ = root.wait().await;
                panic!("nested session did not start");
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };

        let nested_leads_own_session =
            nix::unistd::getsid(Some(Pid::from_raw(nested_pid))) == Ok(Pid::from_raw(nested_pid));
        let stop = stop_process_scopes(
            [scope],
            StopPolicy {
                signal: Signal::SIGTERM as i32,
                grace: Duration::from_secs(2),
            },
        );
        let root_status = tokio::time::timeout(Duration::from_secs(2), root.wait()).await;

        let deadline = Instant::now() + Duration::from_secs(2);
        let nested_gone = loop {
            if signal::kill(Pid::from_raw(nested_pid), None) == Err(nix::errno::Errno::ESRCH) {
                break true;
            }
            if Instant::now() >= deadline {
                let _ = signal::killpg(Pid::from_raw(nested_pid), Signal::SIGKILL);
                break false;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };

        assert!(
            nested_leads_own_session,
            "fixture must cross a setsid boundary"
        );
        stop.expect("terminate process scope");
        let status = root_status
            .expect("process-scope root exits")
            .expect("wait for process-scope root");
        assert!(!status.success());
        assert!(nested_gone, "nested session escaped process-scope cleanup");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn prepared_scope_creates_and_captures_the_scope() {
        let mut command = tokio::process::Command::new("bash");
        command
            .arg("-c")
            .arg("sleep 300 & wait")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let spawn =
            PreparedProcessScope::prepare_tokio(&mut command).expect("prepare process scope");

        let mut child = command.spawn().expect("spawn scope leader");
        let pid = child.id().expect("child pid");
        let scope = spawn.capture(pid).expect("capture process scope");

        assert_eq!(
            nix::unistd::getsid(Some(Pid::from_raw(pid as i32))),
            Ok(Pid::from_raw(pid as i32)),
            "spawned child must lead its Unix session"
        );
        let serialized = serde_json::to_vec(&scope).expect("serialize scope");
        assert_eq!(
            serde_json::from_slice::<ProcessScope>(&serialized).expect("restore scope"),
            scope,
            "durable scope identity must round-trip"
        );

        let cleanup_scope = scope.clone();
        let cleanup = tokio::task::spawn_blocking(move || {
            stop_process_scopes(
                [cleanup_scope],
                StopPolicy {
                    signal: Signal::SIGTERM as i32,
                    grace: Duration::from_secs(2),
                },
            )
        });
        let (status, cleanup) = tokio::join!(child.wait(), cleanup);
        assert!(!status.expect("wait succeeds").success());
        cleanup
            .expect("cleanup task succeeds")
            .expect("scope cleanup succeeds");
        assert!(!scope.is_alive());
    }

    #[test]
    fn mismatched_identity_is_never_signalled() {
        let scope = ProcessScope::unix_session_with_start(std::process::id() as i32, Some(1))
            .expect("capture process scope");
        assert!(!scope.matches_identity());
        scope
            .signal(Signal::SIGKILL as i32)
            .expect("mismatched identity is ignored");
    }

    fn table_entry(pid: i32, parent_pid: i32, process_group: i32) -> ProcessTableEntry {
        ProcessTableEntry {
            pid,
            parent_pid,
            process_group,
            session: 99,
            zombie: false,
        }
    }

    fn descendant_scope(leader_pid: i32, shared_group: Option<i32>) -> ProcessScope {
        ProcessScope {
            backend: ScopeBackend::UnixDescendants {
                leader: ProcessIdentity {
                    pid: leader_pid,
                    start_time: Some(7),
                    boot_id: None,
                },
                shared_group,
            },
        }
    }

    #[test]
    fn a_descendant_scope_spares_the_process_group_it_shares_with_its_caller() {
        // A manager that keeps the caller's controlling terminal runs in the
        // caller's process group. Signalling that group would reach the caller
        // and everything else it started.
        let caller = table_entry(99, 1, 99);
        let leader = table_entry(100, 99, 99);
        let child = table_entry(101, 100, 101);
        let sibling = table_entry(102, 99, 99);
        let table = vec![caller, leader, child, sibling];

        let targets = descendant_scope(leader.pid, Some(caller.process_group)).targets_in(&table);

        assert!(
            !targets.groups.contains(&caller.process_group),
            "the caller's process group must not be a target"
        );
        assert!(
            targets.groups.contains(&child.process_group),
            "a descendant's own process group must be a target"
        );
        assert!(
            targets.processes.contains(&leader.pid),
            "the leader must be signalled directly when its group is not its own"
        );
    }

    #[test]
    fn a_descendant_scope_reaches_a_leader_that_leads_its_own_group() {
        let leader = table_entry(100, 99, 100);
        let child = table_entry(101, 100, 100);
        let table = vec![table_entry(99, 1, 99), leader, child];

        let targets = descendant_scope(leader.pid, None).targets_in(&table);

        assert!(targets.groups.contains(&leader.pid));
        assert!(targets.processes.contains(&leader.pid));
    }

    #[test]
    fn a_descendant_scope_names_children_left_in_the_caller_process_group() {
        // Honcho starts its children with `start_new_session=False`, so they
        // stay in the process group the caller handed the manager.
        let caller = table_entry(99, 1, 99);
        let leader = table_entry(100, 99, 99);
        let child = table_entry(101, 100, 99);
        let table = vec![caller, leader, child];

        let targets = descendant_scope(leader.pid, Some(caller.process_group)).targets_in(&table);

        assert!(
            !targets.groups.contains(&caller.process_group),
            "signalling the shared group would reach the caller"
        );
        assert!(
            targets.processes.contains(&child.pid),
            "a child left in the shared group must be signalled by PID"
        );
    }

    #[test]
    fn a_scope_captured_before_a_reboot_is_not_ours() {
        let live = ProcessScope::descendants_of_current().expect("capture current process");
        assert!(
            live.matches_identity(),
            "a scope captured now must match this boot"
        );

        let start_time = process_start_time(std::process::id() as i32)
            .map_or_else(|| "null".to_string(), |start| start.to_string());
        let stale = format!(
            r#"{{"backend":"unix_descendants","leader":{{"pid":{},"start_time":{start_time},"boot_id":"0000-not-this-boot"}}}}"#,
            std::process::id()
        );
        let stale: ProcessScope = serde_json::from_str(&stale).expect("parse stale scope");

        // Linux measures start times in ticks since boot, so this PID and start
        // time would otherwise look like the running process.
        assert!(
            !stale.matches_identity(),
            "a start time measured against another boot must not identify a process"
        );
    }

    #[test]
    fn constructors_reject_nonpositive_leader_pids() {
        assert!(ProcessScope::unix_session(0).is_err());
        assert!(ProcessScope::legacy_unix_process_group(-1).is_err());
    }

    #[test]
    fn serialized_scope_preserves_the_existing_backend_schema() {
        let scope = ProcessScope::unix_session_with_start(42, Some(7)).expect("create scope");
        let value = serde_json::to_value(&scope).expect("serialize scope");
        assert_eq!(value["backend"], "unix_session");
        assert_eq!(value["leader"]["pid"], 42);
        assert_eq!(value["leader"]["start_time"], 7);
        assert_eq!(
            serde_json::from_value::<ProcessScope>(value).expect("restore serialized scope"),
            scope
        );

        // A document written where a boot identifier carries no meaning.
        let without_boot_id = r#"{"backend":"unix_session","leader":{"pid":42,"start_time":7}}"#;
        let restored =
            serde_json::from_str::<ProcessScope>(without_boot_id).expect("restore scope");
        assert_eq!(restored.leader_pid(), 42);
        assert!(restored.matches_identity() || process_start_time(42) != Some(7));
    }

    #[test]
    fn serialized_scope_rejects_nonpositive_leader_pids() {
        let invalid = r#"{"backend":"unix_session","leader":{"pid":0,"start_time":null}}"#;
        assert!(serde_json::from_str::<ProcessScope>(invalid).is_err());
    }
}
