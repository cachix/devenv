//! Integration tests for `Supervisor::External` mode.
//!
//! Under External, the supervisor task skips the state machine + probes +
//! watcher entirely: it runs `job.start()`, mirrors Ready/Stopping/Exited into
//! `status_rx`, and responds to Stop/Restart commands. Restart policy,
//! readiness probes, watchdog, and file-watch reload are owned by the host
//! manager (process-compose, mprocs, ...).

mod common;

use common::*;
use devenv_processes::{
    HttpGetProbe, HttpProbe, ProcessConfig, ProcessPhase, ReadyConfig, RestartConfig,
    RestartPolicy, Supervisor, WatchdogConfig,
};
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// External-mode process should be reported as `Ready` immediately, since the
/// host owns readiness — no probe runs.
#[tokio::test(flavor = "multi_thread")]
async fn test_external_reports_ready_immediately() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let mut config = ProcessConfig {
            name: "ext-ready".to_string(),
            exec: "sleep".to_string(),
            args: vec!["3600".to_string()],
            supervisor: Supervisor::External,
            // Configure a probe pointing at a port nothing will ever bind.
            // Under Native, this would block in Starting until startup timeout
            // fires. Under External the probe is skipped.
            ready: Some(ReadyConfig {
                http: Some(HttpProbe {
                    get: Some(HttpGetProbe {
                        scheme: "http".to_string(),
                        host: "127.0.0.1".to_string(),
                        port: 1, // unbindable for non-root
                        path: "/".to_string(),
                    }),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        config.restart.on = RestartPolicy::Never;

        let manager = ctx.create_manager();
        manager.start_command(&config, None).await.unwrap();

        let became_ready = wait_for_condition(
            || async { manager.get_phase("ext-ready").await == Some(ProcessPhase::Ready) },
            Duration::from_secs(2),
        )
        .await;
        assert!(
            became_ready,
            "External supervisor must surface Ready immediately, got {:?}",
            manager.get_phase("ext-ready").await
        );

        let _ = manager.stop("ext-ready").await;
    })
    .await
    .expect("test timed out");
}

/// External-mode process must NOT restart on exit, even when RestartPolicy
/// says Always. Restart policy belongs to the host.
#[tokio::test(flavor = "multi_thread")]
async fn test_external_skips_restart_policy() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("exit-fast.sh", "#!/bin/sh\nexit 1\n")
            .await;

        let config = ProcessConfig {
            name: "ext-noretry".to_string(),
            exec: script.to_string_lossy().to_string(),
            supervisor: Supervisor::External,
            restart: RestartConfig {
                on: RestartPolicy::Always,
                ..Default::default()
            },
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager.start_command(&config, None).await.unwrap();

        let reached_exited = wait_for_condition(
            || async { manager.get_phase("ext-noretry").await == Some(ProcessPhase::Exited) },
            Duration::from_secs(5),
        )
        .await;
        assert!(
            reached_exited,
            "External supervisor should reach Exited without restarting, got {:?}",
            manager.get_phase("ext-noretry").await
        );

        // Confirm it stays Exited (no restart loop kicks in late).
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(
            manager.get_phase("ext-noretry").await,
            Some(ProcessPhase::Exited),
            "External must not restart after Exited"
        );
    })
    .await
    .expect("test timed out");
}

/// Watchdog configured but External supervises: no watchdog timeout restart
/// fires, even when the process never pings.
#[tokio::test(flavor = "multi_thread")]
async fn test_external_skips_watchdog() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("silent.sh", "#!/bin/sh\nsleep 3600\n")
            .await;

        let config = ProcessConfig {
            name: "ext-watchdog".to_string(),
            exec: script.to_string_lossy().to_string(),
            supervisor: Supervisor::External,
            watchdog: Some(WatchdogConfig {
                usec: 200_000, // 200ms — would fire fast under Native
                require_ready: false,
            }),
            restart: RestartConfig {
                on: RestartPolicy::Always,
                ..Default::default()
            },
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager.start_command(&config, None).await.unwrap();

        // Give the watchdog plenty of time to (incorrectly) fire under Native;
        // under External there's no watchdog wired so phase stays Ready.
        tokio::time::sleep(Duration::from_secs(2)).await;

        assert_eq!(
            manager.get_phase("ext-watchdog").await,
            Some(ProcessPhase::Ready),
            "External must not honor watchdog; process should remain Ready"
        );

        let _ = manager.stop("ext-watchdog").await;
    })
    .await
    .expect("test timed out");
}

/// Stop command on an External-mode process transitions through Stopping and
/// drives the tail-stop signal.
#[tokio::test(flavor = "multi_thread")]
async fn test_external_stop_command_signals_child() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let config = ProcessConfig {
            name: "ext-stop".to_string(),
            exec: "sleep".to_string(),
            args: vec!["3600".to_string()],
            supervisor: Supervisor::External,
            restart: RestartConfig {
                on: RestartPolicy::Never,
                ..Default::default()
            },
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager.start_command(&config, None).await.unwrap();

        let started = wait_for_process_start(&manager, "ext-stop", Duration::from_secs(2)).await;
        assert!(started, "process should appear in the manager list");

        manager.stop("ext-stop").await.expect("stop should succeed");

        // Manager replaces Active with Stopped after teardown completes.
        let became_stopped = wait_for_condition(
            || async { manager.get_phase("ext-stop").await == Some(ProcessPhase::Stopped) },
            Duration::from_secs(5),
        )
        .await;
        assert!(
            became_stopped,
            "External stop should drive the process through Stopping to Stopped, got {:?}",
            manager.get_phase("ext-stop").await
        );
    })
    .await
    .expect("test timed out");
}

/// Restart command on an External-mode process calls `restart_with_signal`
/// and brings the process back up.
#[tokio::test(flavor = "multi_thread")]
async fn test_external_restart_command_relaunches() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let config = ProcessConfig {
            name: "ext-restart".to_string(),
            exec: "sleep".to_string(),
            args: vec!["3600".to_string()],
            supervisor: Supervisor::External,
            restart: RestartConfig {
                on: RestartPolicy::Never,
                ..Default::default()
            },
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager.start_command(&config, None).await.unwrap();

        let became_ready = wait_for_condition(
            || async { manager.get_phase("ext-restart").await == Some(ProcessPhase::Ready) },
            Duration::from_secs(2),
        )
        .await;
        assert!(became_ready);

        manager
            .restart("ext-restart")
            .await
            .expect("restart should succeed");

        // After restart, still alive; phase should be Ready again.
        let ready_again = wait_for_condition(
            || async { manager.get_phase("ext-restart").await == Some(ProcessPhase::Ready) },
            Duration::from_secs(2),
        )
        .await;
        assert!(
            ready_again,
            "External restart should leave the process Ready, got {:?}",
            manager.get_phase("ext-restart").await
        );

        let _ = manager.stop("ext-restart").await;
    })
    .await
    .expect("test timed out");
}
