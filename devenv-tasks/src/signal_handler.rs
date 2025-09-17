use tokio::signal::unix::{signal, SignalKind};
use tokio_util::sync::CancellationToken;
use tracing::debug;

/// A shared signal handler service that manages signal handling across the entire application.
/// This replaces per-task signal handlers with a single, efficient, centralized handler.
pub struct SignalHandler {
    cancellation_token: CancellationToken,
    _handle: tokio::task::JoinHandle<()>,
}

impl SignalHandler {
    /// Create and start a new signal handler service.
    /// Returns a SignalHandler instance and a CancellationToken that will be triggered
    /// when a signal is received.
    pub fn start() -> Self {
        let cancellation_token = CancellationToken::new();
        let token_clone = cancellation_token.clone();

        let mut sigint = signal(SignalKind::interrupt()).expect("Failed to install SIGINT handler");
        let mut sigterm = signal(SignalKind::terminate()).expect("Failed to install SIGTERM handler");

        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = sigint.recv() => {
                    debug!("Received SIGINT (Ctrl+C), triggering shutdown...");
                    eprintln!("Received SIGINT (Ctrl+C), shutting down gracefully...");
                    token_clone.cancel();
                }
                _ = sigterm.recv() => {
                    debug!("Received SIGTERM, triggering shutdown...");
                    eprintln!("Received SIGTERM, shutting down gracefully...");
                    token_clone.cancel();
                }
            }
        });

        Self {
            cancellation_token,
            _handle: handle,
        }
    }

    /// Get a clone of the cancellation token that will be triggered when a signal is received.
    /// This token can be shared across multiple tasks and components.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }

    /// Check if a shutdown signal has been received.
    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token.is_cancelled()
    }
}

impl Drop for SignalHandler {
    fn drop(&mut self) {
        // The join handle will be aborted when dropped, which is fine
        // since we're shutting down anyway
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_signal_handler_creation() {
        let handler = SignalHandler::start();
        assert!(!handler.is_cancelled());

        let token = handler.cancellation_token();
        assert!(!token.is_cancelled());

        // Test that multiple tokens from the same handler are linked
        let token2 = handler.cancellation_token();
        assert!(!token2.is_cancelled());
    }

    #[tokio::test]
    async fn test_cancellation_token_sharing() {
        let handler = SignalHandler::start();
        let token1 = handler.cancellation_token();
        let token2 = handler.cancellation_token();

        // Manually cancel to test behavior
        handler.cancellation_token.cancel();

        // Small delay to ensure cancellation propagates
        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(token1.is_cancelled());
        assert!(token2.is_cancelled());
        assert!(handler.is_cancelled());
    }

    #[tokio::test]
    async fn test_cancellation_notification() {
        let handler = SignalHandler::start();
        let token = handler.cancellation_token();

        // Spawn a task that waits for cancellation
        let notified = tokio::spawn(async move {
            token.cancelled().await;
            true
        });

        // Cancel after a small delay
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            handler.cancellation_token.cancel();
        });

        // The task should complete when cancelled
        let result = tokio::time::timeout(Duration::from_millis(200), notified).await;
        assert!(result.is_ok());
        assert!(result.unwrap().unwrap());
    }
}
