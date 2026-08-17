//! Integration tests for `SupervisionMode::External` mode.
//!
//! External mode reports lifecycle state but leaves restart, readiness,
//! watchdog, and file-watch policy to the host manager.

mod common;

use common::*;
use devenv_processes::{
    HttpGetProbe, HttpProbe, OnIdle, ProcessConfig, ProcessPhase, ReadyConfig, RestartConfig,
    RestartPolicy, SupervisionMode, WatchConfig, WatchdogConfig,
};
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::test(flavor = "multi_thread")]
async fn test_external_reports_ready_immediately() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let mut config = ProcessConfig {
            name: "ext-ready".to_string(),
            exec: "sleep 3600".to_string(),
            supervisor: SupervisionMode::External,
            // Nothing listens on this port, so a native probe would not pass.
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
            supervisor: SupervisionMode::External,
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
            supervisor: SupervisionMode::External,
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

#[tokio::test(flavor = "multi_thread")]
async fn test_external_exits_even_with_watch_paths() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let config = ProcessConfig {
            name: "ext-watch".to_string(),
            exec: "exit 0".to_string(),
            supervisor: SupervisionMode::External,
            watch: WatchConfig {
                paths: vec![ctx.temp_path().to_path_buf()],
                ..Default::default()
            },
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager.start_command(&config, None).await.unwrap();
        manager
            .run_foreground(
                tokio_util::sync::CancellationToken::new(),
                None,
                OnIdle::Exit,
            )
            .await
            .unwrap();
        assert_eq!(
            manager.get_phase("ext-watch").await,
            Some(ProcessPhase::Exited)
        );
    })
    .await
    .expect("external supervisor stayed alive for a disabled file watcher");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_external_stop_command_signals_child() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let config = ProcessConfig {
            name: "ext-stop".to_string(),
            exec: "sleep 3600".to_string(),
            supervisor: SupervisionMode::External,
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

#[tokio::test(flavor = "multi_thread")]
async fn test_external_restart_command_relaunches() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let config = ProcessConfig {
            name: "ext-restart".to_string(),
            exec: "sleep 3600".to_string(),
            supervisor: SupervisionMode::External,
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
