//! Restart policy integration tests for NativeProcessManager.
//!
//! All tests use event-driven assertions (polling) instead of fixed sleeps
//! to avoid timing-dependent flakiness. Negative assertions (should NOT
//! restart) wait for the process to exit, then poll briefly to confirm
//! no restart occurred.

mod common;

use common::*;
use devenv_processes::{ProcessConfig, RestartPolicy};
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);
const RESTART_TIMEOUT: Duration = Duration::from_secs(10);

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

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for initial start
        let count = wait_for_line_count(&counter_file, "started", 1, STARTUP_TIMEOUT).await;
        assert_eq!(count, 1, "Process should start once");

        // Poll to confirm no restart happened
        let count = wait_for_line_count(&counter_file, "started", 2, Duration::from_secs(2)).await;
        assert_eq!(count, 1, "Process with Never policy should not restart");
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

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Poll until at least 2 starts (proves restart on success)
        let count = wait_for_line_count(&counter_file, "started", 2, RESTART_TIMEOUT).await;
        assert!(
            count >= 2,
            "Process with Always policy should restart on success, got {} starts",
            count
        );

        // Wait for all restarts to complete (1 initial + 3 restarts = 4)
        let count = wait_for_line_count(&counter_file, "started", 4, RESTART_TIMEOUT).await;
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

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Poll until restart detected
        let count = wait_for_line_count(&counter_file, "started", 2, RESTART_TIMEOUT).await;
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

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Poll until restart detected
        let count = wait_for_line_count(&counter_file, "started", 2, RESTART_TIMEOUT).await;
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

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for initial start
        let count = wait_for_line_count(&counter_file, "started", 1, STARTUP_TIMEOUT).await;
        assert_eq!(count, 1, "Process should start once");

        // Poll to confirm no restart (OnFailure + exit 0 = no restart)
        let count = wait_for_line_count(&counter_file, "started", 2, Duration::from_secs(2)).await;
        assert_eq!(
            count, 1,
            "Process with OnFailure policy should NOT restart on exit 0"
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

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for all restarts to complete (1 initial + 3 restarts = 4)
        let count = wait_for_line_count(&counter_file, "started", 4, RESTART_TIMEOUT).await;
        assert_eq!(
            count, 4,
            "Process should start exactly 4 times (1 initial + 3 restarts)"
        );

        // Confirm no more restarts beyond the limit
        let count = wait_for_line_count(&counter_file, "started", 5, Duration::from_secs(2)).await;
        assert_eq!(
            count, 4,
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

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Poll until multiple restarts observed
        let count = wait_for_line_count(&counter_file, "started", 5, RESTART_TIMEOUT).await;
        assert!(
            count >= 5,
            "Process with unlimited restarts should keep restarting, got {} starts",
            count
        );

        // Stop the process
        manager.stop_all().await.expect("Failed to stop");

        // Record count after stop
        let final_count =
            wait_for_line_count(&counter_file, "started", 1, Duration::from_millis(100)).await;

        // Confirm no more restarts after stop
        let after_stop = wait_for_line_count(
            &counter_file,
            "started",
            final_count + 1,
            Duration::from_secs(2),
        )
        .await;
        assert_eq!(
            after_stop, final_count,
            "Process should stop restarting after stop_all"
        );
    })
    .await
    .expect("Test timed out");
}
