//! Integration tests for the cachix daemon client.
//!
//! Uses a mock daemon for reliable, reproducible testing.

use std::sync::Arc;
use std::time::Duration;

use devenv_nix_backend::cachix_daemon::{
    BuildPathCallback, ConnectionParams, DaemonClient, DaemonConnectConfig,
};

mod common;
use common::mock_cachix_daemon::MockCachixDaemon;

fn mock_config(mock: &MockCachixDaemon) -> DaemonConnectConfig {
    DaemonConnectConfig {
        socket_path: mock.socket_path().to_path_buf(),
        connection: ConnectionParams {
            connect_timeout: Duration::from_secs(5),
            operation_timeout: Duration::from_secs(10),
            max_retries: 0,
            reconnect_backoff_ms: 100,
        },
    }
}

async fn setup_mock_and_client() -> (Arc<MockCachixDaemon>, DaemonClient) {
    let mock = Arc::new(
        MockCachixDaemon::start()
            .await
            .expect("Failed to start mock client"),
    );
    let _handler = mock.spawn_handler();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let config = mock_config(&mock);
    let client = DaemonClient::connect(config)
        .await
        .expect("Failed to connect to mock client");

    (mock, client)
}

#[tokio::test]
async fn test_queue_single_path() {
    let (mock, client) = setup_mock_and_client().await;

    let path = "/nix/store/abc123-single-package".to_string();
    client
        .queue_path(path.clone())
        .await
        .expect("queue_path should succeed");

    let result = client
        .wait_for_completion(Duration::from_secs(5))
        .await
        .expect("wait_for_completion should succeed");

    assert_eq!(result.completed, 1, "One path should be completed");
    assert_eq!(result.failed, 0, "No paths should fail");
    assert_eq!(result.queued, 0, "Queue should be empty");

    let received = mock.get_pushed_paths().await;
    assert_eq!(received, vec![path], "Mock should receive the path");
}

#[tokio::test]
async fn test_queue_multiple_paths() {
    let (mock, client) = setup_mock_and_client().await;

    let paths = vec![
        "/nix/store/abc123-package-1.0".to_string(),
        "/nix/store/def456-package-2.0".to_string(),
        "/nix/store/ghi789-package-3.0".to_string(),
    ];

    client
        .queue_paths(paths.clone())
        .await
        .expect("queue_paths should succeed");

    let result = client
        .wait_for_completion(Duration::from_secs(5))
        .await
        .expect("wait_for_completion should succeed");

    assert_eq!(result.completed, 3, "All paths should be completed");
    assert_eq!(result.failed, 0, "No paths should fail");

    let received = mock.get_pushed_paths().await;
    assert_eq!(received, paths, "Mock should receive all paths in order");
}

#[tokio::test]
async fn test_queue_empty_paths_is_noop() {
    let (mock, client) = setup_mock_and_client().await;

    let empty: Vec<String> = vec![];
    client
        .queue_paths(empty)
        .await
        .expect("queue_paths with empty list should succeed");

    // Give a moment for any potential processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    let metrics = client.metrics();
    assert_eq!(metrics.queued, 0, "Nothing should be queued");
    assert_eq!(metrics.completed, 0, "Nothing should be completed");

    let received = mock.get_pushed_paths().await;
    assert!(received.is_empty(), "Mock should not receive any paths");
}

#[tokio::test]
async fn test_metrics_update_during_push() {
    let (_mock, client) = setup_mock_and_client().await;

    // Check initial metrics
    let initial = client.metrics();
    assert_eq!(initial.queued, 0);
    assert_eq!(initial.completed, 0);

    let paths = vec![
        "/nix/store/path-1".to_string(),
        "/nix/store/path-2".to_string(),
    ];
    client.queue_paths(paths).await.unwrap();

    // After completion, metrics should reflect the push
    let result = client
        .wait_for_completion(Duration::from_secs(5))
        .await
        .unwrap();

    assert_eq!(result.completed, 2);
    assert_eq!(result.queued, 0);
    assert_eq!(result.in_progress, 0);
}

#[tokio::test]
async fn test_callback_queues_path() {
    let (mock, client) = setup_mock_and_client().await;

    let callback = client.as_build_callback();

    // Use callback to queue paths (simulating build integration)
    callback
        .on_path_realized("/nix/store/callback-path-1")
        .await
        .expect("on_path_realized should succeed");
    callback
        .on_path_realized("/nix/store/callback-path-2")
        .await
        .expect("on_path_realized should succeed");

    // Note: The callback doesn't update metrics (known limitation), so we can't use
    // wait_for_completion. Instead, poll until the mock receives the paths.
    let mut attempts = 0;
    loop {
        let received = mock.get_pushed_paths().await;
        if received.len() >= 2 {
            assert!(received.contains(&"/nix/store/callback-path-1".to_string()));
            assert!(received.contains(&"/nix/store/callback-path-2".to_string()));
            break;
        }
        attempts += 1;
        if attempts > 50 {
            panic!(
                "Timeout waiting for callback paths, received: {:?}",
                received
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn test_concurrent_queueing() {
    let (mock, client) = setup_mock_and_client().await;
    let client = Arc::new(client);

    let mut handles = vec![];

    // Spawn 5 concurrent tasks, each queueing 2 paths
    for i in 0..5 {
        let client_clone = Arc::clone(&client);
        let handle = tokio::spawn(async move {
            let paths = vec![
                format!("/nix/store/concurrent-{}-a", i),
                format!("/nix/store/concurrent-{}-b", i),
            ];
            client_clone.queue_paths(paths).await
        });
        handles.push(handle);
    }

    // Wait for all queue operations to complete
    for handle in handles {
        handle
            .await
            .expect("Task should not panic")
            .expect("Queueing should succeed");
    }

    // Wait for all paths to be processed
    let result = client
        .wait_for_completion(Duration::from_secs(10))
        .await
        .expect("wait_for_completion should succeed");

    assert_eq!(result.completed, 10, "All 10 paths should complete");
    assert_eq!(result.failed, 0, "No paths should fail");

    let received = mock.get_pushed_paths().await;
    assert_eq!(received.len(), 10, "Mock should receive all 10 paths");
}

#[tokio::test]
async fn test_shutdown_after_queueing() {
    let (mock, client) = setup_mock_and_client().await;

    let paths = vec![
        "/nix/store/shutdown-test-1".to_string(),
        "/nix/store/shutdown-test-2".to_string(),
    ];
    client.queue_paths(paths.clone()).await.unwrap();

    // Wait for completion then shutdown
    client.wait_for_completion(Duration::from_secs(5)).await.ok();
    client.shutdown();

    let received = mock.get_pushed_paths().await;
    assert_eq!(received, paths, "All paths should be pushed before shutdown");
}

#[tokio::test]
async fn test_large_batch_queueing() {
    let (mock, client) = setup_mock_and_client().await;

    // Queue 50 paths in one batch
    let paths: Vec<String> = (0..50)
        .map(|i| format!("/nix/store/batch-path-{}", i))
        .collect();

    client.queue_paths(paths.clone()).await.unwrap();

    let result = client
        .wait_for_completion(Duration::from_secs(30))
        .await
        .expect("wait_for_completion should succeed");

    assert_eq!(result.completed, 50, "All 50 paths should complete");

    let received = mock.get_pushed_paths().await;
    assert_eq!(received.len(), 50);
}
