//! Restart policy integration tests for NativeProcessManager.

mod common;

use common::*;
use devenv_processes::{ProcessConfig, RestartPolicy};
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

// ============================================================================
// Restart Policy Tests
// ============================================================================

/// Test that RestartPolicy::Never does not restart the process
#[tokio::test(flavor = "multi_thread")]
async fn test_restart_never() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("counter.txt");

        // Process that writes to counter file and exits with failure
        let config = ProcessConfig {
            name: "no-restart".to_string(),
            exec: format!(r#"echo "started" >> {}; exit 1"#, counter_file.display()),
            restart: RestartPolicy::Never,
            ..Default::default()
        };

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for process to exit
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Check the counter - should only have 1 entry (no restart)
        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .unwrap_or_default();
        let count = content.lines().count();
        assert_eq!(count, 1, "Process with Never policy should not restart");

        // Wait a bit more to ensure no delayed restart
        tokio::time::sleep(Duration::from_millis(500)).await;
        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .unwrap_or_default();
        let count = content.lines().count();
        assert_eq!(count, 1, "Process should still not have restarted");
    })
    .await
    .expect("Test timed out");
}

/// Test that RestartPolicy::Always restarts the process on success
#[tokio::test(flavor = "multi_thread")]
async fn test_restart_always_on_success() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("counter.txt");

        // Process that writes to counter and exits successfully
        let config = ProcessConfig {
            name: "always-restart".to_string(),
            exec: format!(r#"echo "started" >> {}; exit 0"#, counter_file.display()),
            restart: RestartPolicy::Always,
            max_restarts: Some(3), // Limit restarts for test
            ..Default::default()
        };

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for restarts to occur (should restart up to max_restarts times)
        tokio::time::sleep(Duration::from_secs(2)).await;

        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .unwrap_or_default();
        let count = content.lines().count();

        // Should have started 1 + max_restarts times (initial + 3 restarts = 4)
        assert!(
            count >= 2,
            "Process with Always policy should restart on success, got {} starts",
            count
        );
        assert!(
            count <= 4,
            "Process should respect max_restarts limit, got {} starts",
            count
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

/// Test that RestartPolicy::Always restarts on failure too
#[tokio::test(flavor = "multi_thread")]
async fn test_restart_always_on_failure() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("counter.txt");

        // Process that exits with failure
        let config = ProcessConfig {
            name: "always-fail".to_string(),
            exec: format!(r#"echo "started" >> {}; exit 1"#, counter_file.display()),
            restart: RestartPolicy::Always,
            max_restarts: Some(2),
            ..Default::default()
        };

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for restarts
        tokio::time::sleep(Duration::from_secs(2)).await;

        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .unwrap_or_default();
        let count = content.lines().count();

        // Should restart even on failure
        assert!(
            count >= 2,
            "Process with Always policy should restart on failure, got {} starts",
            count
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

/// Test that RestartPolicy::OnFailure only restarts on non-zero exit
#[tokio::test(flavor = "multi_thread")]
async fn test_restart_on_failure_with_failure() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("counter.txt");

        // Process that exits with failure
        let config = ProcessConfig {
            name: "on-failure".to_string(),
            exec: format!(r#"echo "started" >> {}; exit 1"#, counter_file.display()),
            restart: RestartPolicy::OnFailure,
            max_restarts: Some(2),
            ..Default::default()
        };

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for restarts
        tokio::time::sleep(Duration::from_secs(2)).await;

        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .unwrap_or_default();
        let count = content.lines().count();

        // Should restart on failure
        assert!(
            count >= 2,
            "Process with OnFailure policy should restart on exit 1, got {} starts",
            count
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

/// Test that RestartPolicy::OnFailure does NOT restart on success
#[tokio::test(flavor = "multi_thread")]
async fn test_restart_on_failure_with_success() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("counter.txt");

        // Process that exits successfully
        let config = ProcessConfig {
            name: "on-failure-success".to_string(),
            exec: format!(r#"echo "started" >> {}; exit 0"#, counter_file.display()),
            restart: RestartPolicy::OnFailure,
            max_restarts: Some(3),
            ..Default::default()
        };

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait a bit
        tokio::time::sleep(Duration::from_millis(500)).await;

        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .unwrap_or_default();
        let count = content.lines().count();

        // Should NOT restart on success
        assert_eq!(
            count, 1,
            "Process with OnFailure policy should NOT restart on exit 0"
        );

        // Wait more to ensure no delayed restart
        tokio::time::sleep(Duration::from_millis(500)).await;
        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .unwrap_or_default();
        assert_eq!(
            content.lines().count(),
            1,
            "Process should still not have restarted"
        );
    })
    .await
    .expect("Test timed out");
}

/// Test that max_restarts limits the number of restarts
#[tokio::test(flavor = "multi_thread")]
async fn test_max_restarts_limit() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("counter.txt");

        let config = ProcessConfig {
            name: "max-restarts".to_string(),
            exec: format!(
                r#"echo "started" >> {}; sleep 0.1; exit 1"#,
                counter_file.display()
            ),
            restart: RestartPolicy::Always,
            max_restarts: Some(3),
            ..Default::default()
        };

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait enough time for all restarts to complete
        tokio::time::sleep(Duration::from_secs(3)).await;

        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .unwrap_or_default();
        let count = content.lines().count();

        // Should have exactly 1 initial + 3 restarts = 4 starts
        assert_eq!(
            count, 4,
            "Process should start exactly {} times (1 initial + {} restarts)",
            4, 3
        );

        // Wait more to ensure no more restarts
        tokio::time::sleep(Duration::from_secs(1)).await;
        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .unwrap_or_default();
        assert_eq!(
            content.lines().count(),
            4,
            "Process should not restart beyond max_restarts limit"
        );
    })
    .await
    .expect("Test timed out");
}

/// Test that max_restarts=None allows unlimited restarts (with manual stop)
#[tokio::test(flavor = "multi_thread")]
async fn test_unlimited_restarts() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("counter.txt");

        let config = ProcessConfig {
            name: "unlimited".to_string(),
            exec: format!(
                r#"echo "started" >> {}; sleep 0.1; exit 1"#,
                counter_file.display()
            ),
            restart: RestartPolicy::Always,
            max_restarts: None, // Unlimited
            ..Default::default()
        };

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for several restarts
        tokio::time::sleep(Duration::from_secs(2)).await;

        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .unwrap_or_default();
        let count = content.lines().count();

        // Should have restarted multiple times
        assert!(
            count >= 5,
            "Process with unlimited restarts should keep restarting, got {} starts",
            count
        );

        // Stop the process
        manager.stop_all().await.expect("Failed to stop");

        // Record count after stop
        let final_content = tokio::fs::read_to_string(&counter_file)
            .await
            .unwrap_or_default();
        let final_count = final_content.lines().count();

        // Wait to ensure it stopped
        tokio::time::sleep(Duration::from_millis(500)).await;
        let after_stop_content = tokio::fs::read_to_string(&counter_file)
            .await
            .unwrap_or_default();

        assert_eq!(
            after_stop_content.lines().count(),
            final_count,
            "Process should stop restarting after stop_all"
        );
    })
    .await
    .expect("Test timed out");
}
