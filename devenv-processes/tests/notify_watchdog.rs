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
use devenv_processes::{NotifyConfig, ProcessConfig, WatchdogConfig};
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
        notify: Some(NotifyConfig { enable: true }),
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
        notify: Some(NotifyConfig { enable: true }),
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
        let output_file = ctx.temp_path().join("output.txt");

        // Create a simple script that writes output and sleeps
        let script_content = format!(
            r#"#!/bin/sh
echo "started" > {}
sleep 3600
"#,
            output_file.display()
        );
        let script = ctx.create_script("notify-test.sh", &script_content).await;

        let config = notify_process_config("notify-test", &script);
        let manager = ctx.create_manager_single(config.clone());
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for process to start
        assert!(
            wait_for_file_content(&output_file, "started", STARTUP_TIMEOUT).await,
            "Process should start"
        );

        // Check that notify socket was created
        let notify_socket_path = ctx.state_dir.join("notify/notify-test.sock");
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
        let script = ctx.create_script("env-check.sh", &script_content).await;

        let config = notify_process_config("env-check", &script);
        let manager = ctx.create_manager_single(config.clone());
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for the output file and check NOTIFY_SOCKET was set
        let expected_path = ctx.state_dir.join("notify/env-check.sock");
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
        let script = ctx.create_script("watchdog-env.sh", &script_content).await;

        let config = watchdog_process_config("watchdog-env", &script, 30_000_000, true);
        let manager = ctx.create_manager_single(config.clone());
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
        let output_file = ctx.temp_path().join("output.txt");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" > {}
sleep 3600
"#,
            output_file.display()
        );
        let script = ctx.create_script("cleanup-test.sh", &script_content).await;

        let config = notify_process_config("cleanup-test", &script);
        let manager = ctx.create_manager_single(config.clone());
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for process to start
        assert!(
            wait_for_file_content(&output_file, "started", STARTUP_TIMEOUT).await,
            "Process should start"
        );

        let notify_socket_path = ctx.state_dir.join("notify/cleanup-test.sock");
        assert!(
            notify_socket_path.exists(),
            "Notify socket should exist while process is running"
        );

        // Stop the process
        manager.stop_all().await.unwrap();

        // Poll until socket is cleaned up
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
        let output_file = ctx.temp_path().join("output.txt");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" > {}
sleep 3600
"#,
            output_file.display()
        );
        let script = ctx.create_script("no-notify.sh", &script_content).await;

        // Create config WITHOUT notify
        let config = ProcessConfig {
            name: "no-notify".to_string(),
            exec: script.to_string_lossy().to_string(),
            args: vec![],
            notify: None, // Disabled
            ..Default::default()
        };
        let manager = ctx.create_manager_single(config.clone());
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for process to start
        assert!(
            wait_for_file_content(&output_file, "started", STARTUP_TIMEOUT).await,
            "Process should start"
        );

        // Notify socket should NOT exist
        let notify_socket_path = ctx.state_dir.join("notify/no-notify.sock");
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
        let output_file = ctx.temp_path().join("output.txt");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" > {}
sleep 3600
"#,
            output_file.display()
        );
        let script = ctx.create_script("ready-test.sh", &script_content).await;

        let config = notify_process_config("ready-test", &script);
        let manager = ctx.create_manager_single(config.clone());
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for process to start
        assert!(
            wait_for_file_content(&output_file, "started", STARTUP_TIMEOUT).await,
            "Process should start"
        );

        // Send READY=1 notification
        let notify_socket_path = ctx.state_dir.join("notify/ready-test.sock");
        send_notify(&notify_socket_path, &[NotifyState::Ready]);

        // Brief wait to ensure async notification is processed
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Process should still be running (READY=1 doesn't stop it)
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
        let output_file = ctx.temp_path().join("output.txt");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" > {}
sleep 3600
"#,
            output_file.display()
        );
        let script = ctx.create_script("status-test.sh", &script_content).await;

        let config = notify_process_config("status-test", &script);
        let manager = ctx.create_manager_single(config.clone());
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for process to start
        assert!(
            wait_for_file_content(&output_file, "started", STARTUP_TIMEOUT).await,
            "Process should start"
        );

        // Send STATUS notification
        let notify_socket_path = ctx.state_dir.join("notify/status-test.sock");
        send_notify(
            &notify_socket_path,
            &[NotifyState::Status("Loading configuration...")],
        );

        // Brief wait to ensure async notification is processed
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Process should still be running
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
        let output_file = ctx.temp_path().join("output.txt");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" > {}
sleep 3600
"#,
            output_file.display()
        );
        let script = ctx.create_script("watchdog-ping.sh", &script_content).await;

        // Use a 2 second watchdog timeout, but don't require ready
        let config = watchdog_process_config("watchdog-ping", &script, 2_000_000, false);
        let manager = ctx.create_manager_single(config.clone());
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for process to start
        assert!(
            wait_for_file_content(&output_file, "started", STARTUP_TIMEOUT).await,
            "Process should start"
        );

        let notify_socket_path = ctx.state_dir.join("notify/watchdog-ping.sock");

        // Send watchdog pings every 500ms for 3 seconds (longer than timeout).
        // The 500ms interval is inherent to the test: pings must arrive within
        // the 2s watchdog window to keep the process alive.
        for _ in 0..6 {
            send_notify(&notify_socket_path, &[NotifyState::Watchdog]);
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Process should still be running because we kept pinging
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
        let counter_file = ctx.temp_path().join("start_count.txt");

        // Script increments counter each time it starts
        let script_content = format!(
            r#"#!/bin/sh
if [ -f "{0}" ]; then
    count=$(($(cat "{0}") + 1))
else
    count=1
fi
echo $count > "{0}"
sleep 3600
"#,
            counter_file.display()
        );
        let script = ctx
            .create_script("watchdog-timeout.sh", &script_content)
            .await;

        // Use a 1 second watchdog timeout, require_ready=false so watchdog starts immediately
        let config = watchdog_process_config("watchdog-timeout", &script, 1_000_000, false);
        let manager = ctx.create_manager_single(config.clone());
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for initial start
        assert!(
            wait_for_file_content(&counter_file, "1", STARTUP_TIMEOUT).await,
            "Process should start initially"
        );

        // Don't send any watchdog pings - poll until restart detected
        let count = wait_for_counter_value(&counter_file, 2, RESTART_TIMEOUT).await;
        assert!(
            count >= 2,
            "Process should have restarted due to watchdog timeout, count={}",
            count
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
        let counter_file = ctx.temp_path().join("start_count.txt");

        // Script increments counter each time it starts
        let script_content = format!(
            r#"#!/bin/sh
if [ -f "{0}" ]; then
    count=$(($(cat "{0}") + 1))
else
    count=1
fi
echo $count > "{0}"
sleep 3600
"#,
            counter_file.display()
        );
        let script = ctx
            .create_script("watchdog-ready.sh", &script_content)
            .await;

        // Use a 1 second watchdog timeout with require_ready=true
        let config = watchdog_process_config("watchdog-ready", &script, 1_000_000, true);
        let manager = ctx.create_manager_single(config.clone());
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for initial start
        assert!(
            wait_for_file_content(&counter_file, "1", STARTUP_TIMEOUT).await,
            "Process should start initially"
        );

        // Wait longer than watchdog timeout WITHOUT sending READY=1.
        // This is inherently timing-dependent: we must wait long enough to
        // prove the watchdog did NOT fire. 3x the watchdog timeout is generous.
        let count = wait_for_counter_value(&counter_file, 2, Duration::from_secs(3)).await;
        assert_eq!(
            count, 1,
            "Process should NOT restart without READY=1 when require_ready=true"
        );

        // Now send READY=1 to start watchdog enforcement
        let notify_socket_path = ctx.state_dir.join("notify/watchdog-ready.sock");
        send_notify(&notify_socket_path, &[NotifyState::Ready]);

        // Poll until watchdog-triggered restart
        let count = wait_for_counter_value(&counter_file, 2, RESTART_TIMEOUT).await;
        assert!(
            count >= 2,
            "Process should restart after READY=1 and watchdog timeout, count={}",
            count
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
        let output_file = ctx.temp_path().join("output.txt");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" > {}
sleep 3600
"#,
            output_file.display()
        );
        let script = ctx.create_script("stopping-test.sh", &script_content).await;

        let config = notify_process_config("stopping-test", &script);
        let manager = ctx.create_manager_single(config.clone());
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for process to start
        assert!(
            wait_for_file_content(&output_file, "started", STARTUP_TIMEOUT).await,
            "Process should start"
        );

        // Send STOPPING=1 notification
        let notify_socket_path = ctx.state_dir.join("notify/stopping-test.sock");
        send_notify(&notify_socket_path, &[NotifyState::Stopping]);

        // Brief wait to ensure async notification is processed
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Process should still be running (STOPPING is informational)
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
        let output_file = ctx.temp_path().join("output.txt");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" > {}
sleep 3600
"#,
            output_file.display()
        );
        let script = ctx
            .create_script("reloading-test.sh", &script_content)
            .await;

        let config = notify_process_config("reloading-test", &script);
        let manager = ctx.create_manager_single(config.clone());
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for process to start
        assert!(
            wait_for_file_content(&output_file, "started", STARTUP_TIMEOUT).await,
            "Process should start"
        );

        // Send RELOADING=1 notification
        let notify_socket_path = ctx.state_dir.join("notify/reloading-test.sock");
        send_notify(&notify_socket_path, &[NotifyState::Reloading]);

        // Brief wait to ensure async notification is processed
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Process should still be running (RELOADING is informational)
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
        let output_file = ctx.temp_path().join("output.txt");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" > {}
sleep 3600
"#,
            output_file.display()
        );
        let script = ctx.create_script("multi-state.sh", &script_content).await;

        let config = notify_process_config("multi-state", &script);
        let manager = ctx.create_manager_single(config.clone());
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for process to start
        assert!(
            wait_for_file_content(&output_file, "started", STARTUP_TIMEOUT).await,
            "Process should start"
        );

        // Send multiple states in one notification (READY + STATUS)
        let notify_socket_path = ctx.state_dir.join("notify/multi-state.sock");
        send_notify(
            &notify_socket_path,
            &[NotifyState::Ready, NotifyState::Status("Fully initialized")],
        );

        // Brief wait to ensure async notification is processed
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Process should still be running
        manager
            .stop_all()
            .await
            .expect("Process should still be running after multi-state notification");
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_invalid_notification_does_not_crash() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let output_file = ctx.temp_path().join("output.txt");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" > {}
sleep 3600
"#,
            output_file.display()
        );
        let script = ctx
            .create_script("invalid-notify.sh", &script_content)
            .await;

        let config = notify_process_config("invalid-notify", &script);
        let manager = ctx.create_manager_single(config.clone());
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for process to start
        assert!(
            wait_for_file_content(&output_file, "started", STARTUP_TIMEOUT).await,
            "Process should start"
        );

        let notify_socket_path = ctx.state_dir.join("notify/invalid-notify.sock");

        // Send various malformed messages - manager should handle gracefully
        send_raw_notify(&notify_socket_path, "").unwrap(); // Empty
        send_raw_notify(&notify_socket_path, "INVALID").unwrap(); // No =
        send_raw_notify(&notify_socket_path, "=VALUE").unwrap(); // No key
        send_raw_notify(&notify_socket_path, "UNKNOWN=1\n").unwrap(); // Unknown key
        send_raw_notify(&notify_socket_path, "\x00\x01\x02").unwrap(); // Binary garbage

        // Brief wait to ensure async notification processing completes
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Process should still be running - invalid messages shouldn't crash anything
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
        let counter_file = ctx.temp_path().join("start_count.txt");

        // Script increments counter each time it starts
        let script_content = format!(
            r#"#!/bin/sh
if [ -f "{0}" ]; then
    count=$(($(cat "{0}") + 1))
else
    count=1
fi
echo $count > "{0}"
sleep 3600
"#,
            counter_file.display()
        );
        let script = ctx.create_script("watchdog-max.sh", &script_content).await;

        // Use 500ms watchdog timeout, max 2 restarts
        let mut config = watchdog_process_config("watchdog-max", &script, 500_000, false);
        config.max_restarts = Some(2);

        let manager = ctx.create_manager_single(config.clone());
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for initial start
        assert!(
            wait_for_file_content(&counter_file, "1", STARTUP_TIMEOUT).await,
            "Process should start initially"
        );

        // Poll until at least one restart
        let count = wait_for_counter_value(&counter_file, 2, RESTART_TIMEOUT).await;
        assert!(
            count >= 2,
            "Process should have restarted at least once, count={}",
            count
        );

        // Wait for restarts to exhaust, then confirm max is respected
        let count = wait_for_counter_value(&counter_file, 4, Duration::from_secs(5)).await;
        assert!(
            count <= 3,
            "Process should respect max_restarts=2, but started {} times",
            count
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
        let counter_file = ctx.temp_path().join("start_count.txt");

        // Script increments counter each time it starts
        let script_content = format!(
            r#"#!/bin/sh
if [ -f "{0}" ]; then
    count=$(($(cat "{0}") + 1))
else
    count=1
fi
echo $count > "{0}"
sleep 3600
"#,
            counter_file.display()
        );
        let script = ctx.create_script("delayed-hang.sh", &script_content).await;

        // Use 1 second watchdog timeout with require_ready=true
        let config = watchdog_process_config("delayed-hang", &script, 1_000_000, true);
        let manager = ctx.create_manager_single(config.clone());
        let _job = manager.start_command(&config, None).await.unwrap();

        // Wait for initial start
        assert!(
            wait_for_file_content(&counter_file, "1", STARTUP_TIMEOUT).await,
            "Process should start initially"
        );

        let notify_socket_path = ctx.state_dir.join("notify/delayed-hang.sock");

        // Send READY=1 to start watchdog enforcement
        send_notify(&notify_socket_path, &[NotifyState::Ready]);

        // Send a few watchdog pings to keep it alive.
        // The 400ms interval is inherent to the test: pings must arrive
        // within the 1s watchdog window.
        for _ in 0..3 {
            send_notify(&notify_socket_path, &[NotifyState::Watchdog]);
            tokio::time::sleep(Duration::from_millis(400)).await;
        }

        // Now stop sending pings - simulate a hang.
        // Poll until watchdog-triggered restart.
        let count = wait_for_counter_value(&counter_file, 2, RESTART_TIMEOUT).await;
        assert!(
            count >= 2,
            "Process should restart after delayed hang, count={}",
            count
        );

        manager.stop_all().await.unwrap();
    })
    .await
    .expect("Test timed out");
}
