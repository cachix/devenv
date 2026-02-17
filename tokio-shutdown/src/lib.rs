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
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::info;

/// A graceful shutdown manager for tokio applications
pub struct Shutdown {
    token: CancellationToken,
    last_signal: AtomicI32,
    /// Optional receiver for cleanup completion signal
    cleanup_complete: Mutex<Option<oneshot::Receiver<()>>>,
    /// Hook called before force-exiting (e.g., to restore terminal state)
    pre_exit_hook: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

impl std::fmt::Debug for Shutdown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shutdown")
            .field("token", &self.token)
            .field("last_signal", &self.last_signal)
            .field("cleanup_complete", &"<Mutex>")
            .finish()
    }
}

impl Shutdown {
    /// Create a new Shutdown instance wrapped in Arc
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            token: CancellationToken::new(),
            last_signal: AtomicI32::new(0),
            cleanup_complete: Mutex::new(None),
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

    /// Set the cleanup completion receiver.
    /// When shutdown completes, `wait_for_shutdown_complete()` will await this receiver.
    pub fn set_cleanup_receiver(&self, rx: oneshot::Receiver<()>) {
        *self.cleanup_complete.lock().unwrap() = Some(rx);
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

    /// Install signal handlers for graceful shutdown
    pub async fn install_signals(self: &Arc<Self>) {
        let shutdown = Arc::clone(self);

        tokio::spawn(async move {
            let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
                .expect("Failed to install SIGINT handler");
            let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("Failed to install SIGTERM handler");
            let mut sighup = signal::unix::signal(signal::unix::SignalKind::hangup())
                .expect("Failed to install SIGHUP handler");

            loop {
                let last_signal;

                tokio::select! {
                    _ = sigint.recv() => {
                        last_signal = Signal::SIGINT;
                    }
                    _ = sigterm.recv() => {
                        last_signal = Signal::SIGTERM;
                    }
                    _ = sighup.recv() => {
                        last_signal = Signal::SIGHUP;
                    }
                }

                // If a signal was already received (either from a previous real
                // signal or set by the TUI keyboard handler), this is a repeated
                // interrupt — force-exit immediately.
                if shutdown.last_signal.load(Ordering::Relaxed) != 0 {
                    info!("Received second signal, forcing exit...");
                    shutdown.exit_process();
                }

                info!("Received {:?}, shutting down gracefully...", last_signal);

                // Store the last signal received
                shutdown
                    .last_signal
                    .store(last_signal as i32, Ordering::Relaxed);

                // Trigger shutdown
                shutdown.shutdown();
            }
        });
    }

    /// Wait for shutdown to be requested
    pub async fn wait_for_shutdown(&self) {
        self.token.cancelled().await;
    }

    /// Wait for shutdown to complete (cleanup task finished)
    pub async fn wait_for_shutdown_complete(&self) {
        let rx = self.cleanup_complete.lock().unwrap().take();
        if let Some(rx) = rx {
            // Ignore error (sender dropped means cleanup is done)
            let _ = rx.await;
        }
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
        if let Ok(guard) = self.pre_exit_hook.lock() {
            if let Some(hook) = guard.as_ref() {
                hook();
            }
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

    /// Wait for all tasks to complete, propagating panics
    pub async fn wait_all(&mut self) {
        while let Some(res) = self.join_next().await {
            match res {
                Ok(_) => {}
                Err(err) if err.is_panic() => std::panic::resume_unwind(err.into_panic()),
                Err(err) => panic!("{err}"),
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

        // Set up cleanup channel
        let (cleanup_tx, cleanup_rx) = tokio::sync::oneshot::channel::<()>();
        shutdown.set_cleanup_receiver(cleanup_rx);

        // Spawn cleanup task that sends on completion
        let shutdown_for_task = Arc::clone(&shutdown);
        tokio::spawn(async move {
            shutdown_for_task.cancellation_token().cancelled().await;
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = cleanup_tx.send(());
        });

        // Trigger shutdown
        shutdown.shutdown();

        // This should complete when cleanup sends
        shutdown.wait_for_shutdown_complete().await;
        assert!(shutdown.is_cancelled());
    }

    #[tokio::test]
    async fn test_wait_for_shutdown_complete_no_receiver() {
        let shutdown = Shutdown::new();

        // No cleanup receiver set - should return immediately
        shutdown.shutdown();
        shutdown.wait_for_shutdown_complete().await;
        assert!(shutdown.is_cancelled());
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
