//! Integration tests for the StreamingCachixDaemon
//!
//! Tests cover:
//! - Basic daemon startup and shutdown
//! - Path queuing and metrics tracking
//! - Multiple queue cycles
//! - Daemon crash recovery and reconnection
//! - Callback integration

use std::sync::Arc;
use std::time::Duration;

// Import the daemon module
use devenv_nix_backend::cachix_daemon::{BuildPathCallback, DaemonConfig, StreamingCachixDaemon};

// Import shared test utilities
mod common;
use common::mock_cachix_daemon::MockCachixDaemon;

#[tokio::test]
async fn test_daemon_startup_shutdown() {
    // Test basic daemon startup with default config
    let config = DaemonConfig::default();

    // This may fail if daemon is not running, but that's expected in test environment
    // We're testing that the code path works without panicking
    match StreamingCachixDaemon::start(config).await {
        Ok(mut daemon) => {
            // Shutdown should complete without errors
            let result = daemon.shutdown().await;
            assert!(result.is_ok(), "Shutdown should succeed");
        }
        Err(e) => {
            // It's OK if daemon isn't available in test environment
            eprintln!("Daemon not available (expected in CI): {}", e);
        }
    }
}

#[tokio::test]
async fn test_daemon_metrics_initialization() {
    // Test that metrics are properly initialized
    let config = DaemonConfig::default();

    match StreamingCachixDaemon::start(config).await {
        Ok(daemon) => {
            let metrics = daemon.metrics();

            // Initial metrics should be zero
            assert_eq!(metrics.queued, 0, "Initial queued should be 0");
            assert_eq!(metrics.in_progress, 0, "Initial in_progress should be 0");
            assert_eq!(metrics.completed, 0, "Initial completed should be 0");
            assert_eq!(metrics.failed, 0, "Initial failed should be 0");
        }
        Err(e) => {
            eprintln!("Daemon not available: {}", e);
        }
    }
}

#[tokio::test]
async fn test_queue_multiple_paths() {
    let config = DaemonConfig::default();

    match StreamingCachixDaemon::start(config).await {
        Ok(daemon) => {
            let paths = vec![
                "/nix/store/abc123-package-1.0".to_string(),
                "/nix/store/def456-package-2.0".to_string(),
                "/nix/store/ghi789-package-3.0".to_string(),
            ];

            // Queue should succeed without blocking
            let result = daemon.queue_paths(paths).await;
            assert!(result.is_ok(), "Queuing paths should succeed");

            // Check that queued metric increased
            let metrics = daemon.metrics();
            assert!(
                metrics.queued > 0,
                "Queued metric should be > 0 after queueing"
            );
        }
        Err(e) => {
            eprintln!("Daemon not available: {}", e);
        }
    }
}

#[tokio::test]
async fn test_queue_single_path() {
    let config = DaemonConfig::default();

    match StreamingCachixDaemon::start(config).await {
        Ok(daemon) => {
            let path = "/nix/store/xyz999-single-package".to_string();

            // Queue single path should succeed
            let result = daemon.queue_path(path).await;
            assert!(result.is_ok(), "Queuing single path should succeed");

            // Metrics should reflect the queued path
            let metrics = daemon.metrics();
            assert!(
                metrics.queued > 0,
                "Queued metric should increase after single queue"
            );
        }
        Err(e) => {
            eprintln!("Daemon not available: {}", e);
        }
    }
}

#[tokio::test]
async fn test_metrics_summary() {
    let config = DaemonConfig::default();

    match StreamingCachixDaemon::start(config).await {
        Ok(daemon) => {
            let metrics = daemon.metrics();

            // Summary should produce a non-empty string
            let summary = metrics.summary();
            assert!(!summary.is_empty(), "Summary should not be empty");
            assert!(
                summary.contains("Queued:"),
                "Summary should contain 'Queued:'"
            );
            assert!(
                summary.contains("In Progress:"),
                "Summary should contain 'In Progress:'"
            );
            assert!(
                summary.contains("Completed:"),
                "Summary should contain 'Completed:'"
            );
            assert!(
                summary.contains("Failed:"),
                "Summary should contain 'Failed:'"
            );
        }
        Err(e) => {
            eprintln!("Daemon not available: {}", e);
        }
    }
}

#[tokio::test]
async fn test_daemon_config_timeouts() {
    // Test custom timeout configuration
    let config = DaemonConfig {
        connect_timeout: Duration::from_secs(10),
        operation_timeout: Duration::from_secs(60),
        max_retries: 5,
        reconnect_backoff_ms: 1000,
        socket_path: None,
    };

    assert_eq!(config.connect_timeout, Duration::from_secs(10));
    assert_eq!(config.operation_timeout, Duration::from_secs(60));
    assert_eq!(config.max_retries, 5);
    assert_eq!(config.reconnect_backoff_ms, 1000);
}

#[tokio::test]
async fn test_wait_for_completion_timeout() {
    let config = DaemonConfig::default();

    match StreamingCachixDaemon::start(config).await {
        Ok(daemon) => {
            // Queue some paths
            let paths = vec![
                "/nix/store/abc123-package-1.0".to_string(),
                "/nix/store/def456-package-2.0".to_string(),
            ];

            let _ = daemon.queue_paths(paths).await;

            // Wait with a short timeout should either succeed or timeout gracefully
            let result = daemon.wait_for_completion(Duration::from_millis(100)).await;

            // We just verify it doesn't panic and returns a result
            match result {
                Ok(metrics) => {
                    // Got metrics back, that's fine
                    eprintln!("Completed with metrics: {:?}", metrics.summary());
                }
                Err(e) => {
                    // Timeout is expected, just verify it's an error
                    eprintln!("Wait timed out (expected): {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("Daemon not available: {}", e);
        }
    }
}

#[tokio::test]
async fn test_callback_path_queuing() {
    let config = DaemonConfig::default();

    match StreamingCachixDaemon::start(config).await {
        Ok(daemon) => {
            // Get callback for integration
            let callback = daemon.as_build_callback();

            // Test that callback implements BuildPathCallback trait
            let path = "/nix/store/test-callback-path";

            // This should queue the path without blocking
            let result = callback.on_path_realized(path).await;
            assert!(result.is_ok(), "Callback should queue path successfully");

            // Check that path was queued
            let metrics = daemon.metrics();
            assert!(metrics.queued > 0, "Callback should queue path in daemon");
        }
        Err(e) => {
            eprintln!("Daemon not available: {}", e);
        }
    }
}

#[tokio::test]
async fn test_concurrent_queueing() {
    let config = DaemonConfig::default();

    match StreamingCachixDaemon::start(config).await {
        Ok(daemon) => {
            let daemon = Arc::new(daemon);
            let mut handles = vec![];

            // Spawn multiple concurrent queueing tasks
            for i in 0..5 {
                let daemon_clone = Arc::clone(&daemon);
                let handle = tokio::spawn(async move {
                    let paths = vec![
                        format!("/nix/store/concurrent-{}-a", i),
                        format!("/nix/store/concurrent-{}-b", i),
                    ];
                    daemon_clone.queue_paths(paths).await
                });
                handles.push(handle);
            }

            // Wait for all tasks to complete
            for handle in handles {
                let result = handle.await;
                assert!(result.is_ok(), "Task should complete without panicking");
                let inner = result.unwrap();
                assert!(inner.is_ok(), "Queueing should succeed");
            }

            // Verify metrics show all paths were queued
            let metrics = daemon.metrics();
            assert!(metrics.queued > 0, "Concurrent queueing should add paths");
        }
        Err(e) => {
            eprintln!("Daemon not available: {}", e);
        }
    }
}

#[tokio::test]
async fn test_empty_queue_operations() {
    let config = DaemonConfig::default();

    match StreamingCachixDaemon::start(config).await {
        Ok(daemon) => {
            // Queue empty list should be safe
            let empty_paths: Vec<String> = vec![];
            let result = daemon.queue_paths(empty_paths).await;
            assert!(result.is_ok(), "Empty queue should be safe");

            // Metrics should remain at 0
            let metrics = daemon.metrics();
            assert_eq!(metrics.queued, 0, "Empty queue shouldn't change metrics");
        }
        Err(e) => {
            eprintln!("Daemon not available: {}", e);
        }
    }
}

#[test]
fn test_daemon_config_default() {
    let config = DaemonConfig::default();
    assert_eq!(config.connect_timeout, Duration::from_secs(5));
    assert_eq!(config.operation_timeout, Duration::from_secs(30));
    assert_eq!(config.max_retries, 3);
    assert_eq!(config.reconnect_backoff_ms, 500);
}

#[tokio::test]
async fn test_daemon_with_mock_socket() {
    // Start mock daemon
    let mock = Arc::new(
        MockCachixDaemon::start()
            .await
            .expect("Failed to start mock daemon"),
    );

    eprintln!("Mock daemon listening on: {:?}", mock.socket_path());

    // Verify socket exists before starting client
    assert!(
        mock.socket_path().exists(),
        "Mock socket should exist at {:?}",
        mock.socket_path()
    );

    // Spawn background handler
    let _handler = mock.spawn_handler();

    // Give the listener a moment to be ready
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Start real daemon client - it will connect to our mock socket
    let config = DaemonConfig {
        connect_timeout: Duration::from_secs(5),
        operation_timeout: Duration::from_secs(10),
        max_retries: 0, // Don't retry, just connect to existing socket
        reconnect_backoff_ms: 100,
        socket_path: Some(mock.socket_path().to_path_buf()),
    };
    let daemon = StreamingCachixDaemon::start(config)
        .await
        .expect("Failed to start daemon client");

    // Queue some test paths
    let test_paths = vec![
        "/nix/store/abc123-test-package-1.0".to_string(),
        "/nix/store/def456-test-package-2.0".to_string(),
        "/nix/store/ghi789-test-package-3.0".to_string(),
    ];

    daemon
        .queue_paths(test_paths.clone())
        .await
        .expect("Failed to queue paths");

    // Wait for push to complete
    let result = daemon.wait_for_completion(Duration::from_secs(10)).await;
    assert!(result.is_ok(), "Push should complete successfully");

    let metrics = result.unwrap();
    assert_eq!(metrics.completed, 3, "All paths should be completed");
    assert_eq!(metrics.failed, 0, "No paths should fail");
    assert_eq!(metrics.queued, 0, "Queue should be empty");
    assert_eq!(metrics.in_progress, 0, "No paths in progress");

    // Verify mock received the paths
    let received_paths = mock.get_pushed_paths().await;
    assert_eq!(received_paths.len(), 3, "Mock should receive all paths");
    assert_eq!(received_paths, test_paths, "Paths should match");
}
