use nix::sys::signal::{
    self as nix_signal, SaFlags, SigAction, SigHandler as NixSigHandler, SigSet, Signal,
};
use nix::unistd;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use tokio::signal;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::info;

/// A graceful shutdown manager for tokio applications
#[derive(Debug)]
pub struct Shutdown {
    token: CancellationToken,
    task_count: AtomicUsize,
    shutdown_complete: Notify,
    last_signal: AtomicI32,
}

impl Shutdown {
    /// Create a new Shutdown instance wrapped in Arc
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            token: CancellationToken::new(),
            task_count: AtomicUsize::new(0),
            shutdown_complete: Notify::new(),
            last_signal: AtomicI32::new(0),
        })
    }

    // TODO: use a Result type
    fn register_task(&self) -> bool {
        // Skip registering tasks
        if self.token.is_cancelled() {
            return false;
        }
        self.task_count.fetch_add(1, Ordering::Relaxed);

        true
    }

    fn unregister_task(&self) {
        let remaining = self
            .task_count
            .fetch_sub(1, Ordering::AcqRel)
            .saturating_sub(1);

        if self.token.is_cancelled() && remaining == 0 {
            self.shutdown_complete.notify_waiters();
        }
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
            if !shutdown.register_task() {
                return None;
            }

            tokio::pin!(fut);
            let (result, should_trigger_shutdown) = tokio::select! {
                res = &mut fut => (Some(res), true),
                _ = shutdown.token.cancelled() => (None, false),
            };

            shutdown.unregister_task();

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
            if !shutdown.register_task() {
                return None;
            }

            let result = tokio::select! {
                result = task() => Some(result),
                _ = child_token.cancelled() => {
                    if let Some(cleanup) = cleanup {
                        cleanup().await;
                    }
                    None
                }
            };

            shutdown.unregister_task();

            result
        })
    }

    /// Trigger shutdown
    pub fn shutdown(&self) {
        self.token.cancel();

        // If there are no more tasks, notify that shutdown is complete
        if self.task_count.load(Ordering::Acquire) == 0 {
            self.shutdown_complete.notify_waiters();
        }
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
                        info!("Received SIGINT (Ctrl+C), shutting down gracefully...");
                        last_signal = Signal::SIGINT;
                    }
                    _ = sigterm.recv() => {
                        info!("Received SIGTERM, shutting down gracefully...");
                        last_signal = Signal::SIGTERM;
                    }
                    _ = sighup.recv() => {
                        info!("Received SIGHUP, shutting down gracefully...");
                        last_signal = Signal::SIGHUP;
                    }
                }

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

    /// Wait for shutdown to complete (all tasks finished)
    pub async fn wait_for_shutdown_complete(&self) {
        self.shutdown_complete.notified().await;
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

    /// Get the last signal that was received, if any.
    pub fn last_signal(&self) -> Option<Signal> {
        match self.last_signal.load(Ordering::Relaxed) {
            0 => None,
            i => Signal::try_from(i).ok(),
        }
    }

    /// Restore the default handler for the last received signal and re-raise the signal
    /// to terminate with the correct exit code.
    pub fn exit_process(&self) -> ! {
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

    #[tokio::test]
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
    async fn test_wait_for_shutdown() {
        let shutdown = Shutdown::new();

        // Start a long running task
        let handle = shutdown
            .shutdown_when_done(async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                "done"
            })
            .await;

        // Start shutdown in background
        tokio::spawn({
            let shutdown = Arc::clone(&shutdown);
            async move {
                tokio::time::sleep(Duration::from_millis(25)).await;
                shutdown.shutdown();
            }
        });

        // This should complete when shutdown is done
        shutdown.wait_for_shutdown_complete().await;
        let result = handle.await.unwrap();
        assert_eq!(result, None); // Task was cancelled
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
