//! File watching integration tests for NativeProcessManager.
//!
//! Tests the watch.paths configuration that triggers process restarts
//! when watched files change.
//!
//! All tests use event-driven assertions (polling) instead of fixed sleeps
//! to avoid timing-dependent flakiness. Negative assertions (should NOT
//! restart) use a "canary" pattern: write a non-ignored file after the
//! ignored file to prove the watcher was active throughout.
//!
//! Each test sleeps 1s after the process starts (and thus after the
//! watcher is registered) before modifying watched files. Neither notify
//! nor watchexec expose an API to know when the native OS file watcher
//! has finished setting up, so a short sleep is a somewhat reliable way to
//! avoid a race between watcher registration and the first file write.
//! Another option would be to use the polling watcher instead just for tests.

mod common;

use common::*;
use devenv_processes::ProcessConfig;
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);
const WATCH_TIMEOUT: Duration = Duration::from_secs(10);

// ============================================================================
// Basic File Watching Tests
// ============================================================================

/// Test that process restarts when a watched file changes
#[tokio::test(flavor = "multi_thread")]
async fn test_restart_on_file_change() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("restart_counter.txt");
        let watch_file = ctx.temp_path().join("config.txt");

        // Create initial watch file
        tokio::fs::write(&watch_file, "initial")
            .await
            .expect("Failed to create watch file");

        // Script that increments counter on each start
        let script_content = format!(
            r#"#!/bin/sh
echo "started" >> {}
sleep 3600
"#,
            counter_file.display()
        );
        let script = ctx.create_script("watch_test.sh", &script_content).await;

        let config =
            watch_process_config("watch-restart", &script, vec![watch_file.clone()], vec![]);

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for initial start
        assert!(
            wait_for_file_content(&counter_file, "started", STARTUP_TIMEOUT).await,
            "Process should start initially"
        );

        // Allow the OS file watcher to finish setting up
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Modify the watched file
        tokio::fs::write(&watch_file, "modified")
            .await
            .expect("Failed to modify watch file");

        // Poll until restart detected
        let count = wait_for_line_count(&counter_file, "started", 2, WATCH_TIMEOUT).await;
        assert!(
            count >= 2,
            "Process should restart on file change, got {} starts",
            count
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

/// Test watching a directory for changes
#[tokio::test(flavor = "multi_thread")]
async fn test_watch_directory() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("dir_counter.txt");
        let watch_dir = ctx.temp_path().join("watch_dir");

        // Create watch directory with initial file
        tokio::fs::create_dir_all(&watch_dir)
            .await
            .expect("Failed to create watch dir");
        tokio::fs::write(watch_dir.join("initial.txt"), "content")
            .await
            .expect("Failed to create initial file");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" >> {}
sleep 3600
"#,
            counter_file.display()
        );
        let script = ctx.create_script("dir_watch.sh", &script_content).await;

        let config = watch_process_config("watch-dir", &script, vec![watch_dir.clone()], vec![]);

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for initial start
        assert!(
            wait_for_file_content(&counter_file, "started", STARTUP_TIMEOUT).await,
            "Process should start initially"
        );

        // Allow the OS file watcher to finish setting up
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Create a new file in the watched directory
        tokio::fs::write(watch_dir.join("new_file.txt"), "new content")
            .await
            .expect("Failed to create new file");

        // Poll until restart detected
        let count = wait_for_line_count(&counter_file, "started", 2, WATCH_TIMEOUT).await;
        assert!(
            count >= 2,
            "Process should restart on new file in watched dir, got {} starts",
            count
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

// ============================================================================
// Ignore Pattern Tests
// ============================================================================

/// Test that ignored files don't trigger restart
#[tokio::test(flavor = "multi_thread")]
async fn test_ignore_patterns() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("ignore_counter.txt");
        let watch_dir = ctx.temp_path().join("ignore_watch");

        tokio::fs::create_dir_all(&watch_dir)
            .await
            .expect("Failed to create watch dir");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" >> {}
sleep 3600
"#,
            counter_file.display()
        );
        let script = ctx.create_script("ignore_test.sh", &script_content).await;

        // Watch directory but ignore .log files
        let config = watch_process_config(
            "watch-ignore",
            &script,
            vec![watch_dir.clone()],
            vec!["*.log".to_string()],
        );

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for initial start
        assert!(
            wait_for_file_content(&counter_file, "started", STARTUP_TIMEOUT).await,
            "Process should start initially"
        );

        // Allow the OS file watcher to finish setting up
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Step 1: Prove watcher is active with a canary file
        tokio::fs::write(watch_dir.join("canary.txt"), "canary")
            .await
            .expect("Failed to create canary file");

        let count = wait_for_line_count(&counter_file, "started", 2, WATCH_TIMEOUT).await;
        assert_eq!(
            count, 2,
            "Canary file should trigger exactly one restart (got {} starts)",
            count
        );

        // Step 2: Write ignored file, then a non-ignored trigger file
        tokio::fs::write(watch_dir.join("debug.log"), "log content")
            .await
            .expect("Failed to create log file");
        tokio::fs::write(watch_dir.join("trigger.txt"), "trigger")
            .await
            .expect("Failed to create trigger file");

        let count = wait_for_line_count(&counter_file, "started", 3, WATCH_TIMEOUT).await;

        // Step 3: Assert count is exactly 3 (initial + canary + trigger)
        // If the .log file had triggered a restart, count would be 4
        assert_eq!(
            count, 3,
            "Ignored .log file should NOT trigger restart (got {} starts, expected 3)",
            count
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

/// Test ignoring hidden files and directories
#[tokio::test(flavor = "multi_thread")]
async fn test_ignore_hidden_files() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("hidden_counter.txt");
        let watch_dir = ctx.temp_path().join("hidden_watch");

        tokio::fs::create_dir_all(&watch_dir)
            .await
            .expect("Failed to create watch dir");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" >> {}
sleep 3600
"#,
            counter_file.display()
        );
        let script = ctx.create_script("hidden_test.sh", &script_content).await;

        // Watch directory but ignore hidden files
        let config = watch_process_config(
            "watch-hidden",
            &script,
            vec![watch_dir.clone()],
            vec![".*".to_string()],
        );

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        assert!(
            wait_for_file_content(&counter_file, "started", STARTUP_TIMEOUT).await,
            "Process should start initially"
        );

        // Allow the OS file watcher to finish setting up
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Step 1: Prove watcher is active with a canary file
        tokio::fs::write(watch_dir.join("canary.txt"), "canary")
            .await
            .expect("Failed to create canary file");

        let count = wait_for_line_count(&counter_file, "started", 2, WATCH_TIMEOUT).await;
        assert_eq!(
            count, 2,
            "Canary file should trigger exactly one restart (got {} starts)",
            count
        );

        // Step 2: Write hidden file, then a non-hidden trigger file
        tokio::fs::write(watch_dir.join(".hidden"), "hidden content")
            .await
            .expect("Failed to create hidden file");
        tokio::fs::write(watch_dir.join("trigger.txt"), "trigger")
            .await
            .expect("Failed to create trigger file");

        let count = wait_for_line_count(&counter_file, "started", 3, WATCH_TIMEOUT).await;

        // Step 3: Assert count is exactly 3 (initial + canary + trigger)
        // If the hidden file had triggered a restart, count would be 4
        assert_eq!(
            count, 3,
            "Hidden file should NOT trigger restart (got {} starts, expected 3)",
            count
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

/// Test that extension filter only triggers on matching extensions
#[tokio::test(flavor = "multi_thread")]
async fn test_extension_filter() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("ext_counter.txt");
        let watch_dir = ctx.temp_path().join("ext_watch");

        tokio::fs::create_dir_all(&watch_dir)
            .await
            .expect("Failed to create watch dir");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" >> {}
sleep 3600
"#,
            counter_file.display()
        );
        let script = ctx.create_script("ext_test.sh", &script_content).await;

        // Only watch .rs files
        let config = watch_process_config_with_extensions(
            "watch-ext",
            &script,
            vec![watch_dir.clone()],
            vec!["rs".to_string()],
        );

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        assert!(
            wait_for_file_content(&counter_file, "started", STARTUP_TIMEOUT).await,
            "Process should start initially"
        );

        // Allow the OS file watcher to finish setting up
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Step 1: Prove watcher is active with a .rs file
        tokio::fs::write(watch_dir.join("canary.rs"), "fn main() {}")
            .await
            .expect("Failed to create canary file");

        let count = wait_for_line_count(&counter_file, "started", 2, WATCH_TIMEOUT).await;
        assert_eq!(
            count, 2,
            "Canary .rs file should trigger exactly one restart (got {} starts)",
            count
        );

        // Step 2: Write non-.rs file, then a .rs trigger
        tokio::fs::write(watch_dir.join("readme.txt"), "hello")
            .await
            .expect("Failed to create .txt file");
        tokio::fs::write(watch_dir.join("trigger.rs"), "fn test() {}")
            .await
            .expect("Failed to create trigger .rs file");

        let count = wait_for_line_count(&counter_file, "started", 3, WATCH_TIMEOUT).await;

        // If the .txt file had triggered a restart, count would be 4
        assert_eq!(
            count, 3,
            "Non-.rs file should NOT trigger restart (got {} starts, expected 3)",
            count
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

// ============================================================================
// Multiple Watch Paths Tests
// ============================================================================

/// Test watching multiple paths
#[tokio::test(flavor = "multi_thread")]
async fn test_multiple_watch_paths() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("multi_counter.txt");
        let watch_dir1 = ctx.temp_path().join("watch1");
        let watch_dir2 = ctx.temp_path().join("watch2");

        tokio::fs::create_dir_all(&watch_dir1)
            .await
            .expect("Failed to create watch dir 1");
        tokio::fs::create_dir_all(&watch_dir2)
            .await
            .expect("Failed to create watch dir 2");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" >> {}
sleep 3600
"#,
            counter_file.display()
        );
        let script = ctx.create_script("multi_watch.sh", &script_content).await;

        let config = watch_process_config(
            "watch-multi",
            &script,
            vec![watch_dir1.clone(), watch_dir2.clone()],
            vec![],
        );

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        assert!(
            wait_for_file_content(&counter_file, "started", STARTUP_TIMEOUT).await,
            "Process should start initially"
        );

        // Allow the OS file watcher to finish setting up
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Change file in first directory
        tokio::fs::write(watch_dir1.join("file1.txt"), "content1")
            .await
            .expect("Failed to write to dir1");

        // Poll until restart from dir1
        let count = wait_for_line_count(&counter_file, "started", 2, WATCH_TIMEOUT).await;
        assert!(
            count >= 2,
            "Should restart on change in first watch dir, got {} starts",
            count
        );

        // Change file in second directory
        tokio::fs::write(watch_dir2.join("file2.txt"), "content2")
            .await
            .expect("Failed to write to dir2");

        // Poll until restart from dir2
        let count = wait_for_line_count(&counter_file, "started", count + 1, WATCH_TIMEOUT).await;
        assert!(
            count >= 3,
            "Should also restart on change in second watch dir, got {} starts",
            count
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

// ============================================================================
// Edge Cases
// ============================================================================

/// Test that empty watch paths doesn't set up a watcher
#[tokio::test(flavor = "multi_thread")]
async fn test_empty_watch_paths_no_watcher() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("no_watch_counter.txt");
        let some_file = ctx.temp_path().join("some_file.txt");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" >> {}
sleep 3600
"#,
            counter_file.display()
        );
        let script = ctx.create_script("no_watch.sh", &script_content).await;

        // Config with no watch paths
        let config = ProcessConfig {
            name: "no-watch".to_string(),
            exec: script.to_string_lossy().to_string(),
            ..Default::default()
        };

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        assert!(
            wait_for_file_content(&counter_file, "started", STARTUP_TIMEOUT).await,
            "Process should start"
        );

        let initial_count =
            wait_for_line_count(&counter_file, "started", 1, Duration::from_millis(100)).await;

        // Create a file - should NOT trigger restart (no watch configured)
        tokio::fs::write(&some_file, "content")
            .await
            .expect("Failed to write file");

        // No canary possible here (no watcher), so poll briefly to confirm no change
        let final_count =
            wait_for_line_count(&counter_file, "started", 2, Duration::from_secs(2)).await;

        assert_eq!(
            final_count, initial_count,
            "Without watch paths, file changes should not trigger restart"
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

/// Test rapid file changes (debouncing behavior)
#[tokio::test(flavor = "multi_thread")]
async fn test_rapid_file_changes_debounced() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let counter_file = ctx.temp_path().join("debounce_counter.txt");
        let watch_file = ctx.temp_path().join("rapid.txt");

        tokio::fs::write(&watch_file, "initial")
            .await
            .expect("Failed to create watch file");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" >> {}
sleep 3600
"#,
            counter_file.display()
        );
        let script = ctx.create_script("debounce.sh", &script_content).await;

        let config =
            watch_process_config("watch-debounce", &script, vec![watch_file.clone()], vec![]);

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        assert!(
            wait_for_file_content(&counter_file, "started", STARTUP_TIMEOUT).await,
            "Process should start"
        );

        // Allow the OS file watcher to finish setting up
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Make many rapid changes
        for i in 0..10 {
            tokio::fs::write(&watch_file, format!("change {}", i))
                .await
                .expect("Failed to write");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // Wait for at least one restart
        let count = wait_for_line_count(&counter_file, "started", 2, WATCH_TIMEOUT).await;
        assert!(
            count >= 2,
            "Should restart at least once after file changes, got {} starts",
            count
        );

        // Poll until count stabilizes (same value for several consecutive checks)
        let mut stable_count = count;
        let mut stable_checks = 0;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline && stable_checks < 5 {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let current =
                wait_for_line_count(&counter_file, "started", 1, Duration::from_millis(100)).await;
            if current == stable_count {
                stable_checks += 1;
            } else {
                stable_count = current;
                stable_checks = 0;
            }
        }

        assert!(
            stable_count < 10,
            "Rapid changes should be debounced, got {} restarts",
            stable_count
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}
