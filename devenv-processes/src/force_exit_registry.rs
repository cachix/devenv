//! Tracks live child process scopes so a force-exit can tear them down.
//!
//! Every process scope uses its own Unix session or process group, which means
//! a signal sent to devenv's own process group never reaches it. Orderly shutdown
//! paths stop each process through the manager, and tokio's kill-on-drop catches
//! the direct child if a handle is dropped. Neither runs when the process
//! re-raises a signal to die,
//! which is what [`tokio_shutdown::Shutdown::exit_process`] does on a second
//! Ctrl+C: no destructors, no teardown, and all descendants are reparented to
//! init while still holding their ports and data directories.
//!
//! So the identity of each scope is recorded here at spawn and removed when
//! its child handle is dropped. [`kill_process_scopes`] is what the force-exit path
//! calls to signal whatever is left.

use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard};

use process_wrap::tokio::ChildWrapper;
use tracing::debug;

use crate::process_scope::ProcessScope;

/// Process scopes believed to be alive.
static SCOPES: Mutex<BTreeSet<ProcessScope>> = Mutex::new(BTreeSet::new());

/// Lock the registry, ignoring poisoning.
///
/// A panic while holding this lock leaves a consistent set (the guarded
/// operations are a single insert or remove), and refusing to unlock would mean
/// leaking every process the force-exit path exists to clean up.
fn scopes() -> MutexGuard<'static, BTreeSet<ProcessScope>> {
    SCOPES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Signal every recorded scope and forget it.
///
/// Sends `SIGKILL` to each scope. This runs on the force-exit path, which is
/// reached only after the user has asked twice: the graceful path has already
/// sent its termination signal and is sitting out its grace period, so
/// escalating is what it would have done next, and anything gentler cannot be
/// guaranteed when the caller re-raises and dies immediately afterwards.
///
/// Safe to call from a signal-triggered code path in this codebase because
/// signals are delivered to a normal tokio task rather than to an OS signal
/// handler (see `tokio_shutdown::forward_signals`).
pub fn kill_process_scopes() {
    let scopes = std::mem::take(&mut *scopes());
    if scopes.is_empty() {
        return;
    }
    debug!(count = scopes.len(), "killing process scopes on force exit");
    for scope in scopes {
        let _ = scope.force_kill();
    }
}

/// Number of scopes currently recorded.
pub fn tracked_process_scopes() -> usize {
    scopes().len()
}

/// Removes its scope from the registry when its owner is dropped.
///
/// This is what keeps a recycled pid from being signalled: the handle is dropped
/// as part of reaping the child, and until it is reaped the pid belongs to a
/// zombie, which the kernel will not hand out to anything else.
#[derive(Debug)]
pub struct ProcessScopeGuard(ProcessScope);

impl Drop for ProcessScopeGuard {
    fn drop(&mut self) {
        scopes().remove(&self.0);
    }
}

/// Track a scope until the returned guard is dropped.
///
/// The force-exit hook drains and kills all tracked scopes. Normal lifecycle
/// paths simply drop the guard after reaping the direct child.
pub fn track_process_scope(scope: ProcessScope) -> ProcessScopeGuard {
    scopes().insert(scope.clone());
    ProcessScopeGuard(scope)
}

/// A spawned child whose scope is registered for its handle's lifetime.
///
/// Deliberately holds the guard as a field rather than implementing `Drop`
/// itself, so `into_inner` can hand the child on and drop the registration in
/// the same move: once this wrapper no longer owns the child, nothing would ever
/// deregister it.
#[derive(Debug)]
struct RegisteredChild {
    inner: Box<dyn ChildWrapper>,
    _guard: ProcessScopeGuard,
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

pub(crate) fn track_child(
    inner: Box<dyn ChildWrapper>,
    scope: ProcessScope,
) -> Box<dyn ChildWrapper> {
    Box::new(RegisteredChild {
        inner,
        _guard: track_process_scope(scope),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::errno::Errno;
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    use process_wrap::tokio::{CommandWrap, ProcessSession};

    fn spawn(script: &str) -> Box<dyn ChildWrapper> {
        let mut cmd = CommandWrap::with_new("bash", |c| {
            c.arg("-c").arg(script);
        });
        cmd.wrap(ProcessSession);
        let child = cmd.spawn().expect("spawn");
        let pid = child.id().expect("child pid") as i32;
        track_child(
            child,
            ProcessScope::unix_session(pid).expect("capture scope"),
        )
    }

    fn is_alive(pid: i32) -> bool {
        !matches!(kill(Pid::from_raw(pid), None), Err(Errno::ESRCH))
    }

    /// Both halves live in one test because the registry is process-global:
    /// `kill_process_scopes` drains it, so a sibling test running in the same process
    /// would see its own scope disappear.
    #[tokio::test]
    async fn registers_while_alive_and_kills_the_whole_scope() {
        let baseline = tracked_process_scopes();

        // A handle that is dropped deregisters, so a process that has come and
        // gone is never left behind for `kill_process_scopes` to signal.
        let child = spawn("exit 0");
        assert_eq!(tracked_process_scopes(), baseline + 1);
        drop(child);
        assert_eq!(tracked_process_scopes(), baseline);

        // The payload is backgrounded rather than `exec`ed, so it is a grandchild
        // of this process, the way a service that forks is. Only signalling the
        // scope reaches it.
        let dir = tempfile::tempdir().expect("tempdir");
        let pid_file = dir.path().join("grandchild.pid");
        let mut child = spawn(&format!(
            "sleep 300 & echo $! > {0}.tmp; mv {0}.tmp {0}; wait",
            pid_file.display()
        ));
        assert_eq!(tracked_process_scopes(), baseline + 1);

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

        kill_process_scopes();
        assert_eq!(tracked_process_scopes(), 0, "killed scopes are forgotten");

        // Blocks until the direct child changes state, so no polling interval.
        let status = child.wait().await.expect("wait");
        assert!(!status.success(), "direct child should have been killed");

        // The grandchild is not ours to reap, so wait for the kernel to retire it.
        while is_alive(grandchild) {
            tokio::task::yield_now().await;
        }
    }
}
