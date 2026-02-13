//! Process type integration tests for NativeProcessManager.

mod common;

use common::*;
use devenv_processes::{ProcessConfig, ProcessType, RestartPolicy, SupervisorPhase};
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);
const RESTART_TIMEOUT: Duration = Duration::from_secs(10);

// ============================================================================
// Foreground Process Type Tests
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_foreground_default_restarts_on_failure() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx.create_script("fg-fail.sh", "#!/bin/sh\nexit 1\n").await;

        let config = ProcessConfig {
            name: "foreground-fail".to_string(),
            exec: script.to_string_lossy().to_string(),
            args: vec![],
            process_type: ProcessType::Foreground,
            restart: RestartPolicy::OnFailure,
            max_restarts: Some(2),
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        let reached_gave_up = wait_for_condition(
            || async {
                manager
                    .job_state("foreground-fail")
                    .await
                    .is_some_and(|s| s.phase == SupervisorPhase::GaveUp)
            },
            RESTART_TIMEOUT,
        )
        .await;
        assert!(reached_gave_up, "Process should reach GaveUp phase");

        let status = manager.job_state("foreground-fail").await.unwrap();
        assert_eq!(
            status.restart_count, 2,
            "Should have restarted exactly 2 times"
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_foreground_never_policy() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("fg-never.sh", "#!/bin/sh\nexit 1\n")
            .await;

        let config = ProcessConfig {
            name: "foreground-never".to_string(),
            exec: script.to_string_lossy().to_string(),
            args: vec![],
            process_type: ProcessType::Foreground,
            restart: RestartPolicy::Never,
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        tokio::time::sleep(Duration::from_secs(2)).await;

        let status = manager.job_state("foreground-never").await.unwrap();
        assert_eq!(
            status.restart_count, 0,
            "Foreground with Never policy should not restart"
        );
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_default_process_type_is_foreground() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("default-type.sh", "#!/bin/sh\nexit 1\n")
            .await;

        let config = ProcessConfig {
            name: "default-type".to_string(),
            exec: script.to_string_lossy().to_string(),
            args: vec![],
            restart: RestartPolicy::OnFailure,
            max_restarts: Some(2),
            ..Default::default()
        };

        assert_eq!(
            config.process_type,
            ProcessType::Foreground,
            "Default process_type should be Foreground"
        );

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        let reached_gave_up = wait_for_condition(
            || async {
                manager
                    .job_state("default-type")
                    .await
                    .is_some_and(|s| s.phase == SupervisorPhase::GaveUp)
            },
            RESTART_TIMEOUT,
        )
        .await;
        assert!(reached_gave_up, "Process should reach GaveUp phase");

        let status = manager.job_state("default-type").await.unwrap();
        assert!(
            status.restart_count >= 1,
            "Default process type should behave as Foreground and restart, got {} restarts",
            status.restart_count
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}
