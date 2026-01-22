//! Integration tests for the cachix daemon client.
//!
//! Uses `cachix daemon run --dry-run` for testing without actual uploads.
//!
//! These tests require cachix to be installed and are gated behind the
//! `integration-tests` feature flag. Run with:
//! ```
//! cargo test -p devenv-nix-backend --features integration-tests --test test_cachix_daemon
//! ```

#![cfg(feature = "integration-tests")]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use devenv_nix_backend::cachix_daemon::{
    BuildPathCallback, ConnectionParams, DaemonSpawnConfig, OwnedDaemon,
};

fn cachix_binary() -> PathBuf {
    // Use cachix from PATH for tests
    PathBuf::from("cachix")
}

fn test_socket_path() -> PathBuf {
    let id = std::process::id();
    let thread_id = std::thread::current().id();
    std::env::temp_dir().join(format!("cachix-test-{}-{:?}.sock", id, thread_id))
}

async fn spawn_test_daemon() -> OwnedDaemon {
    let config = DaemonSpawnConfig {
        cache_name: "devenv".to_string(), // Use a known public cache
        socket_path: test_socket_path(),
        binary: cachix_binary(),
        dry_run: true,
    };

    OwnedDaemon::spawn(config, ConnectionParams::default())
        .await
        .expect("Failed to spawn test daemon")
}

#[tokio::test]
async fn test_queue_single_path() {
    let daemon = spawn_test_daemon().await;

    daemon
        .queue_path("/nix/store/abc123-test-package".to_string())
        .await
        .expect("queue_path should succeed");

    let result = daemon
        .wait_for_completion(Duration::from_secs(10))
        .await
        .expect("wait_for_completion should succeed");

    assert_eq!(result.queued, 0, "Queue should be empty");
}

#[tokio::test]
async fn test_queue_multiple_paths() {
    let daemon = spawn_test_daemon().await;

    let paths = vec![
        "/nix/store/abc123-package-1.0".to_string(),
        "/nix/store/def456-package-2.0".to_string(),
        "/nix/store/ghi789-package-3.0".to_string(),
    ];

    daemon
        .queue_paths(paths)
        .await
        .expect("queue_paths should succeed");

    let result = daemon
        .wait_for_completion(Duration::from_secs(10))
        .await
        .expect("wait_for_completion should succeed");

    assert_eq!(result.queued, 0, "Queue should be empty");
}

#[tokio::test]
async fn test_queue_empty_paths_is_noop() {
    let daemon = spawn_test_daemon().await;

    daemon
        .queue_paths(vec![])
        .await
        .expect("queue_paths with empty list should succeed");

    tokio::time::sleep(Duration::from_millis(100)).await;

    let metrics = daemon.metrics();
    assert_eq!(metrics.queued, 0, "Nothing should be queued");
}

#[tokio::test]
async fn test_metrics_initial_state() {
    let daemon = spawn_test_daemon().await;

    let metrics = daemon.metrics();
    assert_eq!(metrics.queued, 0);
    assert_eq!(metrics.in_progress, 0);
    assert_eq!(metrics.completed, 0);
    assert_eq!(metrics.failed, 0);
}

#[tokio::test]
async fn test_callback_queues_path() {
    let daemon = spawn_test_daemon().await;
    let callback = daemon.as_build_callback();

    callback
        .on_path_realized("/nix/store/callback-path-1")
        .await
        .expect("on_path_realized should succeed");

    callback
        .on_path_realized("/nix/store/callback-path-2")
        .await
        .expect("on_path_realized should succeed");

    // Give time for processing
    tokio::time::sleep(Duration::from_millis(500)).await;
}

#[tokio::test]
async fn test_concurrent_queueing() {
    let daemon = Arc::new(spawn_test_daemon().await);

    let mut handles = vec![];

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

    for handle in handles {
        handle
            .await
            .expect("Task should not panic")
            .expect("Queueing should succeed");
    }

    let result = daemon
        .wait_for_completion(Duration::from_secs(10))
        .await
        .expect("wait_for_completion should succeed");

    assert_eq!(result.queued, 0, "Queue should be empty");
}

#[tokio::test]
async fn test_shutdown() {
    let daemon = spawn_test_daemon().await;

    daemon
        .queue_paths(vec![
            "/nix/store/shutdown-test-1".to_string(),
            "/nix/store/shutdown-test-2".to_string(),
        ])
        .await
        .unwrap();

    daemon
        .shutdown(Duration::from_secs(10))
        .await
        .expect("shutdown should succeed");
}

#[tokio::test]
async fn test_large_batch_queueing() {
    let daemon = spawn_test_daemon().await;

    let paths: Vec<String> = (0..50)
        .map(|i| format!("/nix/store/batch-path-{}", i))
        .collect();

    daemon.queue_paths(paths).await.unwrap();

    let result = daemon
        .wait_for_completion(Duration::from_secs(30))
        .await
        .expect("wait_for_completion should succeed");

    assert_eq!(result.queued, 0, "Queue should be empty");
}
