use nix::sys::signal::{
    self as nix_signal, SaFlags, SigAction, SigHandler as NixSigHandler, SigSet,
};
use nix::unistd;

// Re-export Signal for consumers who need to set it manually (e.g., TUI mode)
pub use nix::sys::signal::Signal;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};
use tokio::signal;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tokio_util::task::task_tracker::TaskTrackerToken;
use tracing::info;

/// Outstanding cleanup work registered with [`Shutdown::cleanup_guard`].
///
/// Hold it for as long as the cleanup runs; the shutdown wait completes once
/// every outstanding guard has been dropped.
#[must_use = "hold this guard until its cleanup work has finished"]
#[derive(Debug)]
pub struct CleanupGuard {
    /// Never read: dropping it is what deregisters the cleanup.
    _token: TaskTrackerToken,
}

/// A graceful shutdown manager for tokio applications
pub struct Shutdown {
    token: CancellationToken,
    last_signal: AtomicI32,
    /// Registered cleanup tasks, tracked as guards rather than completion
    /// signals so any number of components can register independently.
    /// Registration and closure are synchronized so shutdown either tracks a
    /// guard or rejects it before it can observe the tracker as empty.
    cleanup: Mutex<CleanupTracker>,
    /// Hook called before force-exiting (e.g., to restore terminal state)
    pre_exit_hook: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

#[derive(Debug)]
struct CleanupTracker {
    tracker: TaskTracker,
    registration_open: bool,
}

impl std::fmt::Debug for Shutdown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shutdown")
            .field("token", &self.token)
            .field("last_signal", &self.last_signal)
            .field("cleanup", &self.cleanup)
            .finish()
    }
}

impl Shutdown {
    /// Create a new Shutdown instance wrapped in Arc
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            token: CancellationToken::new(),
            last_signal: AtomicI32::new(0),
            cleanup: Mutex::new(CleanupTracker {
                tracker: TaskTracker::new(),
                registration_open: true,
            }),
            pre_exit_hook: Mutex::new(None),
        })
    }

    /// Set a hook to be called before force-exiting the process.
    ///
    /// This is called when `exit_process()` is triggered (e.g., on second Ctrl+C).
    /// Use this to restore terminal state or perform other critical cleanup.
    pub fn set_pre_exit_hook<F: Fn() + Send + Sync + 'static>(&self, hook: F) {
        *self.pre_exit_hook.lock().unwrap() = Some(Box::new(hook));
    }

    /// Register cleanup work that shutdown must wait for.
    ///
    /// `wait_for_shutdown_complete()` returns once every guard handed out here
    /// has been dropped. Any number of components can register; with no
    /// registrants the wait returns immediately.
    ///
    /// Once cleanup waiting begins, registration is closed and this returns
    /// `None`. This makes a guard racing with shutdown unambiguous: it is
    /// either tracked by the wait or explicitly rejected.
    #[must_use = "hold the returned guard until cleanup is complete"]
    pub fn cleanup_guard(&self) -> Option<CleanupGuard> {
        let cleanup = self.cleanup.lock().unwrap();
        cleanup.registration_open.then(|| CleanupGuard {
            _token: cleanup.tracker.token(),
        })
    }

    /// Run a task and trigger shutdown when it completes (Send futures only)
    /// The task will be cancelled if shutdown is requested before completion
    pub async fn shutdown_when_done<Fut, T>(
        self: &Arc<Self>,
        fut: Fut,
    ) -> tokio::task::JoinHandle<Option<T>>
    where
        Fut: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let shutdown = Arc::clone(self);

        tokio::spawn(async move {
            if shutdown.is_cancelled() {
                return None;
            }

            tokio::pin!(fut);
            let (result, should_trigger_shutdown) = tokio::select! {
                res = &mut fut => (Some(res), true),
                _ = shutdown.token.cancelled() => (None, false),
            };

            if should_trigger_shutdown {
                shutdown.shutdown();
            }

            result
        })
    }

    /// Run a cancellable task with optional cleanup
    pub async fn cancellable<F, Fut, T, C, CFut>(
        self: &Arc<Self>,
        task: F,
        cleanup: Option<C>,
    ) -> tokio::task::JoinHandle<Option<T>>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
        T: Send + 'static,
        C: FnOnce() -> CFut + Send + 'static,
        CFut: Future<Output = ()> + Send + 'static,
    {
        let shutdown = Arc::clone(self);
        let child_token = self.token.child_token();

        tokio::spawn(async move {
            if shutdown.is_cancelled() {
                return None;
            }

            tokio::select! {
                result = task() => Some(result),
                _ = child_token.cancelled() => {
                    if let Some(cleanup) = cleanup {
                        cleanup().await;
                    }
                    None
                }
            }
        })
    }

    /// Trigger shutdown
    pub fn shutdown(&self) {
        self.token.cancel();
    }

    /// Install signal handlers for graceful shutdown.
    ///
    /// The listener dies with the calling runtime while the process-global OS
    /// handlers remain, swallowing all later signals. Use
    /// [`Self::install_signals_on_thread`] if the runtime is shorter-lived
    /// than the process.
    pub async fn install_signals(self: &Arc<Self>) {
        let shutdown = Arc::clone(self);
        tokio::spawn(forward_signals(
            move |signal| shutdown.handle_signal(signal),
            None,
        ));
    }

    /// Install signal handlers on a dedicated thread that lives for the rest
    /// of the process. Returns once the handlers are registered.
    pub fn install_signals_on_thread(self: &Arc<Self>) {
        let shutdown = Arc::clone(self);
        spawn_signal_listener(move |signal| shutdown.handle_signal(signal));
    }

    /// React to a received signal: the first triggers graceful shutdown, a
    /// repeat (including after a TUI keyboard Ctrl-C) force-exits.
    pub fn handle_signal(&self, signal: Signal) {
        if self.last_signal.load(Ordering::Relaxed) != 0 {
            info!("Received second signal, forcing exit...");
            self.exit_process();
        }

        info!("Received {:?}, shutting down gracefully...", signal);
        self.last_signal.store(signal as i32, Ordering::Relaxed);
        self.shutdown();
    }

    /// Wait for shutdown to be requested
    pub async fn wait_for_shutdown(&self) {
        self.token.cancelled().await;
    }

    /// Wait for shutdown to complete (all registered cleanup finished)
    pub async fn wait_for_shutdown_complete(&self) {
        let cleanup = {
            let mut cleanup = self.cleanup.lock().unwrap();
            cleanup.registration_open = false;
            cleanup.tracker.close();
            cleanup.tracker.clone()
        };
        cleanup.wait().await;
    }

    /// Trigger shutdown and wait for cleanup to finish. Idempotent: safe
    /// to call multiple times — the second call is a no-op (the
    /// cancellation token is already cancelled and every cleanup guard
    /// has been dropped).
    pub async fn shutdown_and_wait(&self) {
        self.shutdown();
        self.wait_for_shutdown_complete().await;
    }

    /// Check if shutdown has been triggered
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Get a clone of the cancellation token.
    /// This token can be shared across multiple tasks and components.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.token.clone()
    }

    /// Create a new ShutdownJoinSet for managing multiple cancellable tasks
    pub fn join_set<T>(self: &Arc<Self>) -> ShutdownJoinSet<T>
    where
        T: 'static,
    {
        ShutdownJoinSet::new(Arc::clone(self))
    }

    /// Get the last signal that was received, if any.
    pub fn last_signal(&self) -> Option<Signal> {
        match self.last_signal.load(Ordering::Relaxed) {
            0 => None,
            i => Signal::try_from(i).ok(),
        }
    }

    /// Handle a user initiated interrupt (e.g., Ctrl+C from keyboard).
    ///
    /// On first call: records SIGINT and triggers graceful shutdown.
    /// On second call (already shutting down): force exits the process.
    pub fn handle_interrupt(&self) {
        if self.is_cancelled() {
            self.exit_process();
        }
        self.set_last_signal(Signal::SIGINT);
        self.shutdown();
    }

    /// Set the last signal manually.
    ///
    /// Used in TUI mode where Ctrl+C is received as a keyboard event rather than
    /// a signal. Setting this ensures the Nix backend knows to interrupt operations.
    pub fn set_last_signal(&self, signal: Signal) {
        self.last_signal.store(signal as i32, Ordering::Relaxed);
    }

    /// Restore the default handler for the last received signal and re-raise the signal
    /// to terminate with the correct exit code.
    pub fn exit_process(&self) -> ! {
        // Run pre-exit hook (e.g., restore terminal state) before killing the process
        if let Ok(guard) = self.pre_exit_hook.lock()
            && let Some(hook) = guard.as_ref()
        {
            hook();
        }

        let signal = self.last_signal().unwrap_or(Signal::SIGTERM);
        let action = SigAction::new(NixSigHandler::SigDfl, SaFlags::empty(), SigSet::empty());
        unsafe {
            nix_signal::sigaction(signal, &action)
                .expect("Failed to restore default signal handler");
            nix_signal::kill(unistd::getpid(), signal).expect("Failed to re-raise signal");
        }

        // Unreachable: something went wrong
        std::process::exit(1);
    }
}

/// Forward SIGINT/SIGTERM/SIGHUP to `notify` from a dedicated thread that
/// lives for the rest of the process. Returns once the handlers are
/// registered.
pub fn spawn_signal_listener<F>(notify: F)
where
    F: FnMut(Signal) + Send + 'static,
{
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();

    std::thread::Builder::new()
        .name("signal_handler".into())
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .build()
                .expect("Failed to build signal runtime")
                .block_on(forward_signals(notify, Some(ready_tx)));
        })
        .expect("Failed to spawn signal thread");

    let _ = ready_rx.recv();
}

/// `ready` is signalled once the listeners are registered.
async fn forward_signals<F>(mut notify: F, ready: Option<std::sync::mpsc::Sender<()>>)
where
    F: FnMut(Signal),
{
    let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
        .expect("Failed to install SIGINT handler");
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
        .expect("Failed to install SIGTERM handler");
    let mut sighup = signal::unix::signal(signal::unix::SignalKind::hangup())
        .expect("Failed to install SIGHUP handler");

    if let Some(ready) = ready {
        let _ = ready.send(());
    }

    loop {
        let signal = tokio::select! {
            _ = sigint.recv() => Signal::SIGINT,
            _ = sigterm.recv() => Signal::SIGTERM,
            _ = sighup.recv() => Signal::SIGHUP,
        };
        notify(signal);
    }
}

/// A JoinSet wrapper that integrates with Shutdown for tracking cancellable tasks
pub struct ShutdownJoinSet<T>
where
    T: 'static,
{
    join_set: JoinSet<Option<T>>,
    shutdown: Arc<Shutdown>,
}

impl<T> ShutdownJoinSet<T>
where
    T: 'static,
{
    fn new(shutdown: Arc<Shutdown>) -> Self {
        Self {
            join_set: JoinSet::new(),
            shutdown,
        }
    }

    /// Spawn a task into this join set
    /// The task is responsible for handling cancellation via the shutdown's cancellation token
    pub fn spawn<F, Fut>(&mut self, task: F) -> &mut Self
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let shutdown = Arc::clone(&self.shutdown);

        self.join_set.spawn(async move {
            if shutdown.is_cancelled() {
                return None;
            }
            Some(task().await)
        });

        self
    }

    /// Spawn a cancellable task into this join set
    pub fn spawn_cancellable<F, Fut, C, CFut>(&mut self, task: F, cleanup: Option<C>) -> &mut Self
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
        T: Send + 'static,
        C: FnOnce() -> CFut + Send + 'static,
        CFut: Future<Output = ()> + Send + 'static,
    {
        let shutdown = Arc::clone(&self.shutdown);
        let child_token = self.shutdown.token.child_token();

        self.join_set.spawn(async move {
            if shutdown.is_cancelled() {
                return None;
            }

            tokio::select! {
                result = task() => Some(result),
                _ = child_token.cancelled() => {
                    if let Some(cleanup) = cleanup {
                        cleanup().await;
                    }
                    None
                }
            }
        });

        self
    }

    /// Wait for the next task to complete
    pub async fn join_next(&mut self) -> Option<Result<Option<T>, tokio::task::JoinError>> {
        self.join_set.join_next().await
    }

    /// Wait for all tasks to complete, propagating panics.
    /// If shutdown is triggered, abort remaining tasks and return.
    pub async fn wait_all(&mut self) {
        let cancel = self.shutdown.cancellation_token();
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    self.join_set.abort_all();
                    // Drain remaining tasks so they're fully cleaned up
                    while self.join_set.join_next().await.is_some() {}
                    break;
                }
                result = self.join_set.join_next() => {
                    match result {
                        Some(Ok(_)) => {}
                        Some(Err(err)) if err.is_panic() => std::panic::resume_unwind(err.into_panic()),
                        Some(Err(err)) => panic!("{err}"),
                        None => break,
                    }
                }
            }
        }
    }

    /// Check if the join set is empty
    pub fn is_empty(&self) -> bool {
        self.join_set.is_empty()
    }

    /// Get the number of tasks in the join set
    pub fn len(&self) -> usize {
        self.join_set.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::sync::{Barrier, oneshot};

    // A signal arriving after some other runtime shut down must still be
    // observed — the process-global OS handlers outlive the listener task.
    #[test]
    fn test_install_signals_on_thread_survives_runtime_drop() {
        let shutdown = Shutdown::new();
        shutdown.install_signals_on_thread();

        // A runtime that comes and goes before the signal arrives.
        {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {});
        }

        nix_signal::kill(unistd::getpid(), Signal::SIGHUP).unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), shutdown.wait_for_shutdown())
                .await
                .expect("signal was not observed after runtime drop");
        });
        assert_eq!(shutdown.last_signal(), Some(Signal::SIGHUP));
    }

    #[tokio::test]
    async fn test_shutdown_when_done() {
        let shutdown = Shutdown::new();

        // Start shutdown in background
        tokio::spawn({
            let shutdown = Arc::clone(&shutdown);
            async move {
                tokio::time::sleep(Duration::from_millis(25)).await;
                shutdown.shutdown();
            }
        });

        // Run task that should be cancelled
        let handle = shutdown
            .shutdown_when_done(async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                "completed"
            })
            .await;

        let result = handle.await.unwrap();
        assert_eq!(result, None); // Task was cancelled
        assert!(shutdown.is_cancelled());
    }

    #[tokio::test]
    async fn test_cancellable_task() {
        let shutdown = Shutdown::new();
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let cancelled_cleanup = cancelled.clone();
        let handle = shutdown
            .cancellable(
                move || async move {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    "task_completed"
                },
                Some(move || {
                    let cancelled = cancelled_cleanup.clone();
                    async move {
                        cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }),
            )
            .await;

        // Start shutdown after a brief delay
        tokio::spawn({
            let shutdown = Arc::clone(&shutdown);
            async move {
                tokio::time::sleep(Duration::from_millis(25)).await;
                shutdown.shutdown();
            }
        });

        let result = handle.await.unwrap();
        assert_eq!(result, None); // Task was cancelled

        assert!(shutdown.is_cancelled());
        assert!(cancelled.load(std::sync::atomic::Ordering::Relaxed));
    }

    // Use start_paused to make time deterministic and avoid race conditions
    #[tokio::test(start_paused = true)]
    async fn test_multiple_tasks() {
        let shutdown = Shutdown::new();

        // Start multiple tasks
        let task1 = shutdown
            .shutdown_when_done(async {
                tokio::time::sleep(Duration::from_millis(30)).await;
                "task1"
            })
            .await;

        let task2 = shutdown
            .shutdown_when_done(async {
                tokio::time::sleep(Duration::from_millis(40)).await;
                "task2"
            })
            .await;

        let task3 = shutdown
            .cancellable(
                || async move {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    "task3"
                },
                None::<fn() -> futures::future::Ready<()>>,
            )
            .await;

        // Trigger shutdown after brief delay
        tokio::spawn({
            let shutdown = Arc::clone(&shutdown);
            async move {
                tokio::time::sleep(Duration::from_millis(15)).await;
                shutdown.shutdown();
            }
        });

        // All tasks should complete
        let (result1, result2, result3) = tokio::try_join!(task1, task2, task3).unwrap();
        // All should be None since they were cancelled
        assert_eq!(result1, None);
        assert_eq!(result2, None);
        assert_eq!(result3, None);
        assert!(shutdown.is_cancelled());
    }

    #[tokio::test]
    async fn test_wait_for_shutdown_complete() {
        let shutdown = Shutdown::new();
        let drained = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let guard = shutdown
            .cleanup_guard()
            .expect("cleanup registration should be open");
        let cleanup = tokio::spawn({
            let shutdown = Arc::clone(&shutdown);
            let drained = drained.clone();
            async move {
                shutdown.cancellation_token().cancelled().await;
                drained.store(true, std::sync::atomic::Ordering::SeqCst);
                drop(guard);
            }
        });

        tokio::time::timeout(Duration::from_secs(5), shutdown.shutdown_and_wait())
            .await
            .expect("cleanup did not finish");

        assert!(
            drained.load(std::sync::atomic::Ordering::SeqCst),
            "wait returned before the cleanup guard was dropped"
        );
        assert!(shutdown.is_cancelled());
        cleanup.await.unwrap();
    }

    #[tokio::test]
    async fn test_wait_for_shutdown_complete_no_registrants() {
        let shutdown = Shutdown::new();

        tokio::time::timeout(Duration::from_secs(5), shutdown.shutdown_and_wait())
            .await
            .expect("wait blocked with no cleanup registered");
        assert!(shutdown.is_cancelled());
    }

    // Virtual time: the elapsed timeout below asserts the wait stays pending
    // while a guard is outstanding, without spending real time.
    #[tokio::test(start_paused = true)]
    async fn test_wait_for_shutdown_complete_waits_for_every_guard() {
        let shutdown = Shutdown::new();
        let first = shutdown
            .cleanup_guard()
            .expect("cleanup registration should be open");
        let second = shutdown
            .cleanup_guard()
            .expect("cleanup registration should be open");

        shutdown.shutdown();
        drop(first);

        assert!(
            tokio::time::timeout(
                Duration::from_secs(1),
                shutdown.wait_for_shutdown_complete()
            )
            .await
            .is_err(),
            "wait completed while a cleanup guard was still outstanding"
        );

        drop(second);
        tokio::time::timeout(
            Duration::from_secs(1),
            shutdown.wait_for_shutdown_complete(),
        )
        .await
        .expect("cleanup did not finish");
    }

    #[tokio::test]
    async fn test_shutdown_and_wait_is_idempotent() {
        let shutdown = Shutdown::new();
        drop(
            shutdown
                .cleanup_guard()
                .expect("cleanup registration should be open"),
        );

        for _ in 0..3 {
            tokio::time::timeout(Duration::from_secs(5), shutdown.shutdown_and_wait())
                .await
                .expect("repeated shutdown_and_wait blocked");
        }
    }

    #[tokio::test]
    async fn test_cleanup_guard_is_rejected_after_waiting_begins() {
        let shutdown = Shutdown::new();

        shutdown.shutdown_and_wait().await;

        assert!(
            shutdown.cleanup_guard().is_none(),
            "cleanup registered after shutdown could not be waited for"
        );
    }

    #[tokio::test]
    async fn test_cleanup_guard_racing_shutdown_is_tracked_or_rejected() {
        let shutdown = Shutdown::new();
        let start = Arc::new(Barrier::new(3));
        let (registered_tx, registered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();

        let registration = tokio::spawn({
            let shutdown = Arc::clone(&shutdown);
            let start = Arc::clone(&start);
            async move {
                start.wait().await;
                match shutdown.cleanup_guard() {
                    Some(guard) => {
                        registered_tx.send(true).unwrap();
                        release_rx.await.unwrap();
                        drop(guard);
                    }
                    None => registered_tx.send(false).unwrap(),
                }
            }
        });
        let mut wait = tokio::spawn({
            let shutdown = Arc::clone(&shutdown);
            let start = Arc::clone(&start);
            async move {
                start.wait().await;
                shutdown.shutdown_and_wait().await;
            }
        });

        start.wait().await;

        if registered_rx.await.unwrap() {
            assert!(
                tokio::time::timeout(Duration::from_millis(50), &mut wait)
                    .await
                    .is_err(),
                "shutdown completed before its racing cleanup guard was released"
            );
            release_tx.send(()).unwrap();
        }

        wait.await.unwrap();
        registration.await.unwrap();
    }

    #[tokio::test]
    async fn test_shutdown_when_done_triggers_shutdown() {
        let shutdown = Shutdown::new();

        // Task that completes after a short delay
        let handle = shutdown
            .shutdown_when_done(async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                "completed"
            })
            .await;

        // Wait for the task to complete
        let result = handle.await.unwrap();
        assert_eq!(result, Some("completed")); // Task completed successfully

        // Shutdown should have been triggered automatically
        assert!(shutdown.is_cancelled());
    }

    #[tokio::test]
    async fn test_shutdown_when_done_cancelled_before_completion() {
        let shutdown = Shutdown::new();

        // Long running task
        let handle = shutdown
            .shutdown_when_done(async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                "never_reached"
            })
            .await;

        // Trigger shutdown before task completes
        tokio::spawn({
            let shutdown = Arc::clone(&shutdown);
            async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                shutdown.shutdown();
            }
        });

        // Task should be cancelled
        let result = handle.await.unwrap();
        assert_eq!(result, None); // Task was cancelled before completion
        assert!(shutdown.is_cancelled());
    }

    #[tokio::test]
    async fn test_task_error_propagation() {
        let shutdown = Shutdown::new();

        // Task that returns an error
        let handle = shutdown
            .shutdown_when_done(async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Result::<&str, &str>::Err("task failed")
            })
            .await;

        // Wait for the task to complete
        let result = handle.await.unwrap();
        assert_eq!(result, Some(Err("task failed"))); // Error should be propagated

        // Shutdown should have been triggered automatically
        assert!(shutdown.is_cancelled());
    }

    #[tokio::test]
    async fn test_cancellable_task_error_propagation() {
        let shutdown = Shutdown::new();

        // Task that returns an error
        let handle = shutdown
            .cancellable(
                || async {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    Result::<&str, &str>::Err("cancellable task failed")
                },
                None::<fn() -> futures::future::Ready<()>>,
            )
            .await;

        // Wait for the task to complete
        let result = handle.await.unwrap();
        assert_eq!(result, Some(Err("cancellable task failed"))); // Error should be propagated
    }

    #[tokio::test]
    async fn test_cancellation_token_sharing() {
        let shutdown = Shutdown::new();
        let token1 = shutdown.cancellation_token();
        let token2 = shutdown.cancellation_token();

        // Manually trigger shutdown to test behavior
        shutdown.shutdown();

        // Small delay to ensure cancellation propagates
        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(token1.is_cancelled());
        assert!(token2.is_cancelled());
        assert!(shutdown.is_cancelled());
    }

    #[tokio::test]
    async fn test_cancellation_notification() {
        let shutdown = Shutdown::new();
        let token = shutdown.cancellation_token();

        // Spawn a task that waits for cancellation
        let notified = tokio::spawn(async move {
            token.cancelled().await;
            true
        });

        // Cancel after a small delay
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            shutdown.shutdown();
        });

        // The task should complete when cancelled
        let result = tokio::time::timeout(Duration::from_millis(200), notified).await;
        assert!(result.is_ok());
        assert!(result.unwrap().unwrap());
    }
}
