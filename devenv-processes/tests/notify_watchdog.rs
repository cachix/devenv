//! Integration tests for notify socket and watchdog functionality.
//!
//! Tests use event-driven assertions (polling) instead of fixed sleeps
//! where possible. Brief 100ms sleeps after sending notifications are
//! kept because they just ensure async notification processing completes
//! before checking "process still alive" — these are not timing-sensitive.
//! Watchdog ping intervals are inherent to test design (must be spaced
//! relative to the watchdog timeout).

mod common;

use common::*;
use devenv_processes::{ProcessConfig, ReadyConfig, SupervisorPhase, WatchdogConfig};
use sd_notify::NotifyState;
use std::os::unix::net::UnixDatagram;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);
const RESTART_TIMEOUT: Duration = Duration::from_secs(10);

/// Mutex to serialize access to NOTIFY_SOCKET env var across parallel tests
static NOTIFY_ENV_MUTEX: Mutex<()> = Mutex::new(());

/// Helper to create a process config with notify enabled
fn notify_process_config(name: &str, script_path: &std::path::Path) -> ProcessConfig {
    ProcessConfig {
        name: name.to_string(),
        exec: script_path.to_string_lossy().to_string(),
        args: vec![],
        ready: Some(ReadyConfig {
            notify: true,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Helper to create a process config with watchdog enabled
fn watchdog_process_config(
    name: &str,
    script_path: &std::path::Path,
    watchdog_usec: u64,
    require_ready: bool,
) -> ProcessConfig {
    ProcessConfig {
        name: name.to_string(),
        exec: script_path.to_string_lossy().to_string(),
        args: vec![],
        ready: Some(ReadyConfig {
            notify: true,
            ..Default::default()
        }),
        watchdog: Some(WatchdogConfig {
            usec: watchdog_usec,
            require_ready,
        }),
        ..Default::default()
    }
}

/// Send a notification to a socket path using sd-notify.
/// Uses a mutex to safely manipulate the NOTIFY_SOCKET env var.
fn send_notify(socket_path: &std::path::Path, states: &[NotifyState]) {
    let _guard = NOTIFY_ENV_MUTEX.lock().unwrap();
    // SAFETY: Protected by mutex, no concurrent access to env var
    unsafe {
        std::env::set_var("NOTIFY_SOCKET", socket_path);
    }
    sd_notify::notify(false, states).expect("Should send notification");
    unsafe {
        std::env::remove_var("NOTIFY_SOCKET");
    }
}

/// Send a raw message to a notify socket (for testing malformed messages)
fn send_raw_notify(socket_path: &std::path::Path, message: &str) -> std::io::Result<()> {
    let sock = UnixDatagram::unbound()?;
    sock.send_to(message.as_bytes(), socket_path)?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_notify_socket_created() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("ntfy.sh", "#!/bin/sh\nsleep 3600\n")
            .await;

        let config = notify_process_config("ntfy", &script);
        let manager = ctx.create_manager();
        let _job = manager.start_command(&config, None).await.unwrap();

        let notify_socket_path = ctx.state_dir.join("notify/ntfy.sock");
        assert!(
            notify_socket_path.exists(),
            "Notify socket should exist at {}",
            notify_socket_path.display()
        );

        manager.stop_all().await.unwrap();
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_notify_socket_env_var_set() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let output_file = ctx.temp_path().join("env_output.txt");

        // Create a script that writes NOTIFY_SOCKET env var to a file
        let script_content = format!(
            r#"#!/bin/sh
echo "NOTIFY_SOCKET=$NOTIFY_SOCKET" > {}
sleep 3600
"#,
            output_file.display()
        );
        let script = ctx.create_script("env.sh", &script_content).await;

        let config = notify_process_config("env", &script);
        let manager = ctx.create_manager();
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for the output file and check NOTIFY_SOCKET was set
        let expected_path = ctx.state_dir.join("notify/env.sock");
        let expected = format!("NOTIFY_SOCKET={}", expected_path.display());
        assert!(
            wait_for_file_content(&output_file, &expected, STARTUP_TIMEOUT).await,
            "NOTIFY_SOCKET should be set to {}",
            expected_path.display()
        );

        manager.stop_all().await.unwrap();
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_watchdog_usec_env_var_set() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let output_file = ctx.temp_path().join("watchdog_env.txt");

        // Create a script that writes WATCHDOG_USEC env var to a file
        let script_content = format!(
            r#"#!/bin/sh
echo "WATCHDOG_USEC=$WATCHDOG_USEC" > {}
sleep 3600
"#,
            output_file.display()
        );
        let script = ctx.create_script("wdenv.sh", &script_content).await;

        let config = watchdog_process_config("wdenv", &script, 30_000_000, true);
        let manager = ctx.create_manager();
        let _job = manager.start_command(&config, None).await.unwrap();

        // Check that WATCHDOG_USEC was set
        assert!(
            wait_for_file_content(&output_file, "WATCHDOG_USEC=30000000", STARTUP_TIMEOUT).await,
            "WATCHDOG_USEC should be set to 30000000"
        );

        manager.stop_all().await.unwrap();
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_notify_socket_cleanup_on_stop() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("clean.sh", "#!/bin/sh\nsleep 3600\n")
            .await;

        let config = notify_process_config("clean", &script);
        let manager = ctx.create_manager();
        let _job = manager.start_command(&config, None).await.unwrap();

        let notify_socket_path = ctx.state_dir.join("notify/clean.sock");
        assert!(
            notify_socket_path.exists(),
            "Notify socket should exist while process is running"
        );

        manager.stop_all().await.unwrap();

        let path = notify_socket_path.clone();
        assert!(
            wait_for_condition(
                || {
                    let path = path.clone();
                    async move { !path.exists() }
                },
                STARTUP_TIMEOUT
            )
            .await,
            "Notify socket should be cleaned up after stop"
        );
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_no_notify_socket_when_disabled() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("nosd.sh", "#!/bin/sh\nsleep 3600\n")
            .await;

        // Create config WITHOUT notify (no ready config)
        let config = ProcessConfig {
            name: "nosd".to_string(),
            exec: script.to_string_lossy().to_string(),
            args: vec![],
            ..Default::default()
        };
        let manager = ctx.create_manager();
        let _job = manager.start_command(&config, None).await.unwrap();

        assert!(
            wait_for_condition(
                || async {
                    manager.job_state("nosd").await.is_some_and(|s| {
                        s.phase == SupervisorPhase::Starting || s.phase == SupervisorPhase::Ready
                    })
                },
                STARTUP_TIMEOUT
            )
            .await,
            "Supervisor should be running"
        );

        let notify_socket_path = ctx.state_dir.join("notify/nosd.sock");
        assert!(
            !notify_socket_path.exists(),
            "Notify socket should NOT exist when notify is disabled"
        );

        manager.stop_all().await.unwrap();
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_manager_receives_ready_notification() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx.create_script("rdy.sh", "#!/bin/sh\nsleep 3600\n").await;

        let config = notify_process_config("rdy", &script);
        let manager = ctx.create_manager();
        let _job = manager.start_command(&config, None).await.unwrap();

        let notify_socket_path = ctx.state_dir.join("notify/rdy.sock");
        send_notify(&notify_socket_path, &[NotifyState::Ready]);

        tokio::time::sleep(Duration::from_millis(100)).await;

        manager
            .stop_all()
            .await
            .expect("Process should still be running to stop");
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_manager_receives_status_notification() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("stat.sh", "#!/bin/sh\nsleep 3600\n")
            .await;

        let config = notify_process_config("stat", &script);
        let manager = ctx.create_manager();
        let _job = manager.start_command(&config, None).await.unwrap();

        let notify_socket_path = ctx.state_dir.join("notify/stat.sock");
        send_notify(
            &notify_socket_path,
            &[NotifyState::Status("Loading configuration...")],
        );

        tokio::time::sleep(Duration::from_millis(100)).await;

        manager
            .stop_all()
            .await
            .expect("Process should still be running after STATUS");
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_watchdog_ping_resets_timer() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("wdping.sh", "#!/bin/sh\nsleep 3600\n")
            .await;

        // Use a 2 second watchdog timeout, but don't require ready
        let config = watchdog_process_config("wdping", &script, 2_000_000, false);
        let manager = ctx.create_manager();
        let _job = manager.start_command(&config, None).await.unwrap();

        let notify_socket_path = ctx.state_dir.join("notify/wdping.sock");

        // Send watchdog pings every 500ms for 3 seconds (longer than timeout).
        // The 500ms interval is inherent to the test: pings must arrive within
        // the 2s watchdog window to keep the process alive.
        for _ in 0..6 {
            send_notify(&notify_socket_path, &[NotifyState::Watchdog]);
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        manager
            .stop_all()
            .await
            .expect("Process should still be running when watchdog pings are sent");
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_watchdog_timeout_triggers_restart() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("wdtout.sh", "#!/bin/sh\nsleep 3600\n")
            .await;

        // Use a 1 second watchdog timeout, require_ready=false so watchdog starts immediately
        let config = watchdog_process_config("wdtout", &script, 1_000_000, false);
        let manager = ctx.create_manager();
        manager.start_command(&config, None).await.unwrap();

        // Don't send any watchdog pings — wait for at least one restart
        let restarted = wait_for_condition(
            || async {
                manager
                    .job_state("wdtout")
                    .await
                    .is_some_and(|s| s.restart_count >= 1)
            },
            RESTART_TIMEOUT,
        )
        .await;
        assert!(
            restarted,
            "Process should have restarted due to watchdog timeout"
        );

        manager.stop_all().await.unwrap();
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_watchdog_requires_ready_before_enforcing() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("wdrdy.sh", "#!/bin/sh\nsleep 3600\n")
            .await;

        // Use a 1 second watchdog timeout with require_ready=true
        let config = watchdog_process_config("wdrdy", &script, 1_000_000, true);
        let manager = ctx.create_manager();
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait longer than watchdog timeout WITHOUT sending READY=1.
        // This is inherently timing-dependent: we must wait long enough to
        // prove the watchdog did NOT fire. 3x the watchdog timeout is generous.
        tokio::time::sleep(Duration::from_secs(3)).await;
        let status = manager.job_state("wdrdy").await.unwrap();
        assert_eq!(
            status.restart_count, 0,
            "Process should NOT restart without READY=1 when require_ready=true"
        );

        // Now send READY=1 to start watchdog enforcement
        let notify_socket_path = ctx.state_dir.join("notify/wdrdy.sock");
        send_notify(&notify_socket_path, &[NotifyState::Ready]);

        let restarted = wait_for_condition(
            || async {
                manager
                    .job_state("wdrdy")
                    .await
                    .is_some_and(|s| s.restart_count >= 1)
            },
            RESTART_TIMEOUT,
        )
        .await;
        assert!(
            restarted,
            "Process should restart after READY=1 and watchdog timeout"
        );

        manager.stop_all().await.unwrap();
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_manager_receives_stopping_notification() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("stop.sh", "#!/bin/sh\nsleep 3600\n")
            .await;

        let config = notify_process_config("stop", &script);
        let manager = ctx.create_manager();
        let _job = manager.start_command(&config, None).await.unwrap();

        let notify_socket_path = ctx.state_dir.join("notify/stop.sock");
        send_notify(&notify_socket_path, &[NotifyState::Stopping]);

        tokio::time::sleep(Duration::from_millis(100)).await;

        manager
            .stop_all()
            .await
            .expect("Process should still be running after STOPPING");
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_manager_receives_reloading_notification() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("rload.sh", "#!/bin/sh\nsleep 3600\n")
            .await;

        let config = notify_process_config("rload", &script);
        let manager = ctx.create_manager();
        let _job = manager.start_command(&config, None).await.unwrap();

        let notify_socket_path = ctx.state_dir.join("notify/rload.sock");
        send_notify(&notify_socket_path, &[NotifyState::Reloading]);

        tokio::time::sleep(Duration::from_millis(100)).await;

        manager
            .stop_all()
            .await
            .expect("Process should still be running after RELOADING");
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_multiple_states_in_one_message() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("multi.sh", "#!/bin/sh\nsleep 3600\n")
            .await;

        let config = notify_process_config("multi", &script);
        let manager = ctx.create_manager();
        let _job = manager.start_command(&config, None).await.unwrap();

        let notify_socket_path = ctx.state_dir.join("notify/multi.sock");
        send_notify(
            &notify_socket_path,
            &[NotifyState::Ready, NotifyState::Status("Fully initialized")],
        );

        tokio::time::sleep(Duration::from_millis(100)).await;

        manager
            .stop_all()
            .await
            .expect("Process should still be running after multi notification");
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_invalid_notification_does_not_crash() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx.create_script("inv.sh", "#!/bin/sh\nsleep 3600\n").await;

        let config = notify_process_config("inv", &script);
        let manager = ctx.create_manager();
        let _job = manager.start_command(&config, None).await.unwrap();

        let notify_socket_path = ctx.state_dir.join("notify/inv.sock");

        send_raw_notify(&notify_socket_path, "").unwrap();
        send_raw_notify(&notify_socket_path, "INVALID").unwrap();
        send_raw_notify(&notify_socket_path, "=VALUE").unwrap();
        send_raw_notify(&notify_socket_path, "UNKNOWN=1\n").unwrap();
        send_raw_notify(&notify_socket_path, "\x00\x01\x02").unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        manager
            .stop_all()
            .await
            .expect("Process should still be running after invalid notifications");
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_watchdog_respects_max_restarts() {
    timeout(Duration::from_secs(60), async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("wdmax.sh", "#!/bin/sh\nsleep 3600\n")
            .await;

        // Use 1s watchdog timeout, max 2 restarts
        let mut config = watchdog_process_config("wdmax", &script, 1_000_000, false);
        config.restart.max = Some(2);

        let manager = ctx.create_manager();
        manager.start_command(&config, None).await.unwrap();

        // Wait for the supervisor to give up after exhausting restarts.
        // With max_restarts=2, the watchdog fires 3 times: 2 restarts then GaveUp.
        let gave_up = wait_for_condition(
            || async {
                manager
                    .job_state("wdmax")
                    .await
                    .is_some_and(|s| s.phase == devenv_processes::SupervisorPhase::GaveUp)
            },
            RESTART_TIMEOUT,
        )
        .await;
        assert!(gave_up, "Supervisor should give up after max_restarts");

        let status = manager.job_state("wdmax").await.unwrap();
        assert_eq!(
            status.restart_count, 2,
            "Expected exactly 2 restarts, got {}",
            status.restart_count
        );

        manager.stop_all().await.unwrap();
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_delayed_hang_detection() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let script = ctx
            .create_script("hang.sh", "#!/bin/sh\nsleep 3600\n")
            .await;

        // Use 1 second watchdog timeout with require_ready=true
        let config = watchdog_process_config("hang", &script, 1_000_000, true);
        let manager = ctx.create_manager();
        let _job = manager.start_command(&config, None).await.unwrap();

        let notify_socket_path = ctx.state_dir.join("notify/hang.sock");

        send_notify(&notify_socket_path, &[NotifyState::Ready]);

        // Send a few watchdog pings to keep it alive.
        // The 400ms interval is inherent to the test: pings must arrive
        // within the 1s watchdog window.
        for _ in 0..3 {
            send_notify(&notify_socket_path, &[NotifyState::Watchdog]);
            tokio::time::sleep(Duration::from_millis(400)).await;
        }

        // Stop sending pings to simulate a hang
        let restarted = wait_for_condition(
            || async {
                manager
                    .job_state("hang")
                    .await
                    .is_some_and(|s| s.restart_count >= 1)
            },
            RESTART_TIMEOUT,
        )
        .await;
        assert!(restarted, "Process should restart after delayed hang");

        manager.stop_all().await.unwrap();
    })
    .await
    .expect("Test timed out");
}
