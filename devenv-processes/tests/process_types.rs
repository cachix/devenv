//! Process type integration tests for NativeProcessManager.

mod common;

use common::*;
use devenv_processes::{ProcessConfig, ProcessType, RestartPolicy};
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

// ============================================================================
// Foreground Process Type Tests
// ============================================================================

/// Test that foreground process (default) behaves as before - restarts on failure
#[tokio::test(flavor = "multi_thread")]
async fn test_foreground_default_restarts_on_failure() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("counter.txt");

        // Foreground process with OnFailure policy (default behavior)
        let config = ProcessConfig {
            name: "foreground-fail".to_string(),
            exec: format!(r#"echo "started" >> {}; exit 1"#, counter_file.display()),
            process_type: ProcessType::Foreground,
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

        // Should have started multiple times (1 initial + restarts)
        assert!(
            count >= 2,
            "Foreground process should restart on failure, got {} starts",
            count
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

/// Test that foreground process with Never policy doesn't restart
#[tokio::test(flavor = "multi_thread")]
async fn test_foreground_never_policy() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("counter.txt");

        let config = ProcessConfig {
            name: "foreground-never".to_string(),
            exec: format!(r#"echo "started" >> {}; exit 1"#, counter_file.display()),
            process_type: ProcessType::Foreground,
            restart: RestartPolicy::Never,
            ..Default::default()
        };

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait
        tokio::time::sleep(Duration::from_millis(500)).await;

        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .unwrap_or_default();
        let count = content.lines().count();
        assert_eq!(count, 1, "Foreground with Never policy should not restart");
    })
    .await
    .expect("Test timed out");
}

/// Test that default process type is Foreground
#[tokio::test(flavor = "multi_thread")]
async fn test_default_process_type_is_foreground() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("counter.txt");

        // Don't specify process_type - should default to Foreground
        let config = ProcessConfig {
            name: "default-type".to_string(),
            exec: format!(r#"echo "started" >> {}; exit 1"#, counter_file.display()),
            // process_type not specified - should default to Foreground
            restart: RestartPolicy::OnFailure,
            max_restarts: Some(2),
            ..Default::default()
        };

        // Verify the default
        assert_eq!(
            config.process_type,
            ProcessType::Foreground,
            "Default process_type should be Foreground"
        );

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for restarts - if default is Foreground, it should restart
        tokio::time::sleep(Duration::from_secs(2)).await;

        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .unwrap_or_default();
        let count = content.lines().count();

        assert!(
            count >= 2,
            "Default process type should behave as Foreground and restart, got {} starts",
            count
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}
