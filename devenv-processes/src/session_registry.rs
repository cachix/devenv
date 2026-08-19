//! Tracks the sessions of live child processes so a force-exit can tear them down.
//!
//! Every process is spawned into its own session (`SpawnOptions { session: true }`,
//! see [`crate::command::build_command`]), which means a signal sent to devenv's
//! own process group never reaches it. Orderly shutdown paths stop each process
//! through the manager, and tokio's kill-on-drop catches the direct child if a
//! handle is dropped. Neither runs when the process re-raises a signal to die,
//! which is what [`tokio_shutdown::Shutdown::exit_process`] does on a second
//! Ctrl+C: no destructors, no teardown, and the whole session is reparented to
//! init still holding its ports and data directories.
//!
//! So the pid of each session leader is recorded here at spawn and removed when
//! its child handle is dropped. [`kill_sessions`] is what the force-exit path
//! calls to signal whatever is left.

use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use process_wrap::tokio::{ChildWrapper, CommandWrap, CommandWrapper};
use tracing::{debug, trace};

/// Session leader pids of processes believed to be alive.
///
/// A session leader's pgid equals its pid, so the pid doubles as the process
/// group to signal.
static SESSIONS: Mutex<BTreeSet<i32>> = Mutex::new(BTreeSet::new());

/// Lock the registry, ignoring poisoning.
///
/// A panic while holding this lock leaves a consistent set (the guarded
/// operations are a single insert or remove), and refusing to unlock would mean
/// leaking every process the force-exit path exists to clean up.
fn sessions() -> MutexGuard<'static, BTreeSet<i32>> {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Signal every recorded session and forget it.
///
/// Sends `SIGKILL` to each session's process group. This runs on the force-exit
/// path, which is reached only after the user has asked twice: the graceful path
/// has already sent `SIGTERM` and is sitting out its grace period, so escalating
/// is what it would have done next, and anything gentler cannot be guaranteed
/// when the caller re-raises and dies immediately afterwards.
///
/// Safe to call from a signal-triggered code path in this codebase because
/// signals are delivered to a normal tokio task rather than to an OS signal
/// handler (see `tokio_shutdown::forward_signals`).
pub fn kill_sessions() {
    let pids = std::mem::take(&mut *sessions());
    if pids.is_empty() {
        return;
    }
    debug!(count = pids.len(), "killing process sessions on force exit");
    for pid in pids {
        if let Err(e) = killpg(Pid::from_raw(pid), Signal::SIGKILL) {
            trace!(pid, error = %e, "failed to kill process session");
        }
    }
}

/// Number of sessions currently recorded.
pub fn tracked_sessions() -> usize {
    sessions().len()
}

/// Removes its pid from the registry when the child it accompanies is dropped.
///
/// This is what keeps a recycled pid from being signalled: the handle is dropped
/// as part of reaping the child, and until it is reaped the pid belongs to a
/// zombie, which the kernel will not hand out to anything else.
#[derive(Debug)]
struct SessionGuard(i32);

impl Drop for SessionGuard {
    fn drop(&mut self) {
        sessions().remove(&self.0);
    }
}

/// A spawned child whose session is registered for the lifetime of the handle.
///
/// Deliberately holds the guard as a field rather than implementing `Drop`
/// itself, so `into_inner` can hand the child on and drop the registration in
/// the same move: once this wrapper no longer owns the child, nothing would ever
/// deregister it.
#[derive(Debug)]
struct RegisteredChild {
    inner: Box<dyn ChildWrapper>,
    _guard: SessionGuard,
}

impl ChildWrapper for RegisteredChild {
    fn inner(&self) -> &dyn ChildWrapper {
        self.inner.as_ref()
    }

    fn inner_mut(&mut self) -> &mut dyn ChildWrapper {
        self.inner.as_mut()
    }

    fn into_inner(self: Box<Self>) -> Box<dyn ChildWrapper> {
        self.inner
    }
}

/// Records each spawned child's session so [`kill_sessions`] can reach it.
///
/// Apply after the wrapper that creates the session; `wrap_child` runs in the
/// order wrappers were added, and the pid is the same either way because
/// `ChildWrapper::id` delegates down the stack.
#[derive(Clone, Copy, Debug)]
pub struct SessionRegistrar;

impl CommandWrapper for SessionRegistrar {
    fn wrap_child(
        &mut self,
        inner: Box<dyn ChildWrapper>,
        _core: &CommandWrap,
    ) -> std::io::Result<Box<dyn ChildWrapper>> {
        // A child reaped this early has no session left to track, so pass it on
        // untouched rather than registering a pid that can be recycled.
        let Some(pid) = inner.id().and_then(|id| i32::try_from(id).ok()) else {
            return Ok(inner);
        };

        sessions().insert(pid);
        trace!(pid, "registered process session");
        Ok(Box::new(RegisteredChild {
            inner,
            _guard: SessionGuard(pid),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::errno::Errno;
    use nix::sys::signal::kill;
    use process_wrap::tokio::ProcessSession;

    fn spawn(script: &str) -> Box<dyn ChildWrapper> {
        let mut cmd = CommandWrap::with_new("bash", |c| {
            c.arg("-c").arg(script);
        });
        // Same wrapper order as `build_command` sets up for real processes.
        cmd.wrap(ProcessSession);
        cmd.wrap(SessionRegistrar);
        cmd.spawn().expect("spawn")
    }

    fn is_alive(pid: i32) -> bool {
        !matches!(kill(Pid::from_raw(pid), None), Err(Errno::ESRCH))
    }

    /// Both halves live in one test because the registry is process-global:
    /// `kill_sessions` drains it, so a sibling test running in the same process
    /// would see its own session disappear.
    #[tokio::test]
    async fn registers_while_alive_and_kills_the_whole_session() {
        let baseline = tracked_sessions();

        // A handle that is dropped deregisters, so a process that has come and
        // gone is never left behind for `kill_sessions` to signal.
        let child = spawn("exit 0");
        assert_eq!(tracked_sessions(), baseline + 1);
        drop(child);
        assert_eq!(tracked_sessions(), baseline);

        // The payload is backgrounded rather than `exec`ed, so it is a grandchild
        // of this process, the way a service that forks is. Only signalling the
        // session reaches it.
        let dir = tempfile::tempdir().expect("tempdir");
        let pid_file = dir.path().join("grandchild.pid");
        let mut child = spawn(&format!(
            "sleep 300 & echo $! > {0}.tmp; mv {0}.tmp {0}; wait",
            pid_file.display()
        ));
        assert_eq!(tracked_sessions(), baseline + 1);

        // `mv` is atomic, so the file appears only once it holds the full pid.
        let grandchild: i32 = loop {
            if let Ok(contents) = std::fs::read_to_string(&pid_file)
                && let Ok(pid) = contents.trim().parse()
            {
                break pid;
            }
            tokio::task::yield_now().await;
        };
        assert!(is_alive(grandchild), "grandchild should be running");

        kill_sessions();
        assert_eq!(tracked_sessions(), 0, "killed sessions are forgotten");

        // Blocks until the direct child changes state, so no polling interval.
        let status = child.wait().await.expect("wait");
        assert!(!status.success(), "direct child should have been killed");

        // The grandchild is not ours to reap, so wait for the kernel to retire it.
        while is_alive(grandchild) {
            tokio::task::yield_now().await;
        }
    }
}
