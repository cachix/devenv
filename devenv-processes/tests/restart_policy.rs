//! Restart policy integration tests for NativeProcessManager.
//!
//! All tests use the `job_state()` API to observe supervisor phase and
//! restart count, avoiding filesystem-based communication.

mod common;

use common::*;
use devenv_processes::{ProcessConfig, RestartConfig, RestartPolicy, SupervisorPhase};
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
        let script = ctx
            .create_script("exit_fail.sh", "#!/bin/sh\nexit 1\n")
            .await;

        let config = ProcessConfig {
            name: "no-restart".to_string(),
            exec: script.to_string_lossy().to_string(),
            args: vec![],
            restart: RestartConfig {
                on: RestartPolicy::Never,
                ..Default::default()
            },
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        tokio::time::sleep(Duration::from_secs(2)).await;
        let status = manager.job_state("no-restart").await.unwrap();
        assert_eq!(
            status.restart_count, 0,
            "Process with Never policy should not restart"
        );
    })
    .await
    .expect("Test timed out");
}

/// Test that RestartPolicy::Always restarts the process on success
#[tokio::test(flavor = "multi_thread")]
async fn test_restart_always_on_success() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx.create_script("exit_ok.sh", "#!/bin/sh\nexit 0\n").await;

        let config = ProcessConfig {
            name: "always-restart".to_string(),
            exec: script.to_string_lossy().to_string(),
            args: vec![],
            restart: RestartConfig {
                on: RestartPolicy::Always,
                max: Some(3),
            },
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        let gave_up = wait_for_condition(
            || async {
                manager
                    .job_state("always-restart")
                    .await
                    .is_some_and(|s| s.phase == SupervisorPhase::GaveUp)
            },
            RESTART_TIMEOUT,
        )
        .await;
        assert!(gave_up, "Supervisor should give up after max_restarts");

        let status = manager.job_state("always-restart").await.unwrap();
        assert_eq!(status.restart_count, 3);
    })
    .await
    .expect("Test timed out");
}

/// Test that RestartPolicy::Always restarts on failure too
#[tokio::test(flavor = "multi_thread")]
async fn test_restart_always_on_failure() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("exit_fail.sh", "#!/bin/sh\nexit 1\n")
            .await;

        let config = ProcessConfig {
            name: "always-fail".to_string(),
            exec: script.to_string_lossy().to_string(),
            args: vec![],
            restart: RestartConfig {
                on: RestartPolicy::Always,
                max: Some(2),
            },
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        let gave_up = wait_for_condition(
            || async {
                manager
                    .job_state("always-fail")
                    .await
                    .is_some_and(|s| s.phase == SupervisorPhase::GaveUp)
            },
            RESTART_TIMEOUT,
        )
        .await;
        assert!(gave_up, "Supervisor should give up after max_restarts");

        let status = manager.job_state("always-fail").await.unwrap();
        assert_eq!(status.restart_count, 2);
    })
    .await
    .expect("Test timed out");
}

/// Test that RestartPolicy::OnFailure only restarts on non-zero exit
#[tokio::test(flavor = "multi_thread")]
async fn test_restart_on_failure_with_failure() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("exit_fail.sh", "#!/bin/sh\nexit 1\n")
            .await;

        let config = ProcessConfig {
            name: "on-failure".to_string(),
            exec: script.to_string_lossy().to_string(),
            args: vec![],
            restart: RestartConfig {
                on: RestartPolicy::OnFailure,
                max: Some(2),
            },
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        let gave_up = wait_for_condition(
            || async {
                manager
                    .job_state("on-failure")
                    .await
                    .is_some_and(|s| s.phase == SupervisorPhase::GaveUp)
            },
            RESTART_TIMEOUT,
        )
        .await;
        assert!(gave_up, "Supervisor should give up after max_restarts");

        let status = manager.job_state("on-failure").await.unwrap();
        assert_eq!(status.restart_count, 2);
    })
    .await
    .expect("Test timed out");
}

/// Test that RestartPolicy::OnFailure does NOT restart on success
#[tokio::test(flavor = "multi_thread")]
async fn test_restart_on_failure_with_success() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx.create_script("exit_ok.sh", "#!/bin/sh\nexit 0\n").await;

        let config = ProcessConfig {
            name: "on-failure-success".to_string(),
            exec: script.to_string_lossy().to_string(),
            args: vec![],
            restart: RestartConfig {
                on: RestartPolicy::OnFailure,
                max: Some(3),
            },
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        tokio::time::sleep(Duration::from_secs(2)).await;
        let status = manager.job_state("on-failure-success").await.unwrap();
        assert_eq!(
            status.restart_count, 0,
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
        let script = ctx
            .create_script("exit_fail.sh", "#!/bin/sh\nexit 1\n")
            .await;

        let config = ProcessConfig {
            name: "max-restarts".to_string(),
            exec: script.to_string_lossy().to_string(),
            args: vec![],
            restart: RestartConfig {
                on: RestartPolicy::Always,
                max: Some(3),
            },
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        let gave_up = wait_for_condition(
            || async {
                manager
                    .job_state("max-restarts")
                    .await
                    .is_some_and(|s| s.phase == SupervisorPhase::GaveUp)
            },
            RESTART_TIMEOUT,
        )
        .await;
        assert!(gave_up, "Supervisor should give up after max_restarts");

        let status = manager.job_state("max-restarts").await.unwrap();
        assert_eq!(status.restart_count, 3);
    })
    .await
    .expect("Test timed out");
}

/// Test that max_restarts=None allows unlimited restarts (with manual stop)
#[tokio::test(flavor = "multi_thread")]
async fn test_unlimited_restarts() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("exit_delay.sh", "#!/bin/sh\nsleep 0.1\nexit 1\n")
            .await;

        let config = ProcessConfig {
            name: "unlimited".to_string(),
            exec: script.to_string_lossy().to_string(),
            args: vec![],
            restart: RestartConfig {
                on: RestartPolicy::Always,
                max: None,
            },
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        let reached = wait_for_condition(
            || async {
                manager
                    .job_state("unlimited")
                    .await
                    .is_some_and(|s| s.restart_count >= 4)
            },
            RESTART_TIMEOUT,
        )
        .await;
        assert!(
            reached,
            "Process with unlimited restarts should keep restarting"
        );

        manager.stop_all().await.expect("Failed to stop");

        tokio::time::sleep(Duration::from_millis(500)).await;
        let count_after_stop = manager
            .job_state("unlimited")
            .await
            .map(|s| s.restart_count)
            .unwrap_or(0);

        tokio::time::sleep(Duration::from_secs(2)).await;
        let count_later = manager
            .job_state("unlimited")
            .await
            .map(|s| s.restart_count)
            .unwrap_or(0);

        assert_eq!(
            count_after_stop, count_later,
            "Process should stop restarting after stop_all"
        );
    })
    .await
    .expect("Test timed out");
}
