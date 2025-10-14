//! File watching integration tests for NativeProcessManager.
//!
//! Tests the watch.paths configuration that triggers process restarts
//! when watched files change.

mod common;

use common::*;
use devenv_processes::ProcessConfig;
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

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

        // Give file watcher time to set up
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Modify the watched file
        tokio::fs::write(&watch_file, "modified")
            .await
            .expect("Failed to modify watch file");

        // Wait for restart (should see second "started" in counter file)
        tokio::time::sleep(Duration::from_secs(2)).await;

        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .expect("Failed to read counter");
        let start_count = content.lines().filter(|l| l.contains("started")).count();

        assert!(
            start_count >= 2,
            "Process should restart on file change, got {} starts",
            start_count
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

        // Give file watcher time to set up
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Create a new file in the watched directory
        tokio::fs::write(watch_dir.join("new_file.txt"), "new content")
            .await
            .expect("Failed to create new file");

        // Wait for restart
        tokio::time::sleep(Duration::from_secs(2)).await;

        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .expect("Failed to read counter");
        let start_count = content.lines().filter(|l| l.contains("started")).count();

        assert!(
            start_count >= 2,
            "Process should restart on new file in watched dir, got {} starts",
            start_count
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

        // Give watcher time to set up
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Record current count
        let initial_content = tokio::fs::read_to_string(&counter_file)
            .await
            .expect("Failed to read counter");
        let initial_count = initial_content
            .lines()
            .filter(|l| l.contains("started"))
            .count();

        // Create an ignored file (.log) - should NOT trigger restart
        tokio::fs::write(watch_dir.join("debug.log"), "log content")
            .await
            .expect("Failed to create log file");

        // Wait a bit - no restart should happen
        tokio::time::sleep(Duration::from_secs(2)).await;

        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .expect("Failed to read counter");
        let count_after_log = content.lines().filter(|l| l.contains("started")).count();

        assert_eq!(
            count_after_log, initial_count,
            "Ignored .log file should NOT trigger restart"
        );

        // Now create a non-ignored file - should trigger restart
        tokio::fs::write(watch_dir.join("config.txt"), "config content")
            .await
            .expect("Failed to create config file");

        tokio::time::sleep(Duration::from_secs(2)).await;

        let final_content = tokio::fs::read_to_string(&counter_file)
            .await
            .expect("Failed to read counter");
        let final_count = final_content
            .lines()
            .filter(|l| l.contains("started"))
            .count();

        assert!(
            final_count > initial_count,
            "Non-ignored file should trigger restart, got {} starts (was {})",
            final_count,
            initial_count
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

        tokio::time::sleep(Duration::from_millis(500)).await;

        let initial_content = tokio::fs::read_to_string(&counter_file)
            .await
            .expect("Failed to read counter");
        let initial_count = initial_content
            .lines()
            .filter(|l| l.contains("started"))
            .count();

        // Create a hidden file - should NOT trigger restart
        tokio::fs::write(watch_dir.join(".hidden"), "hidden content")
            .await
            .expect("Failed to create hidden file");

        tokio::time::sleep(Duration::from_secs(2)).await;

        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .expect("Failed to read counter");
        let count_after_hidden = content.lines().filter(|l| l.contains("started")).count();

        assert_eq!(
            count_after_hidden, initial_count,
            "Hidden file should NOT trigger restart"
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

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Change file in first directory
        tokio::fs::write(watch_dir1.join("file1.txt"), "content1")
            .await
            .expect("Failed to write to dir1");

        tokio::time::sleep(Duration::from_secs(2)).await;

        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .expect("Failed to read counter");
        let count_after_dir1 = content.lines().filter(|l| l.contains("started")).count();

        assert!(
            count_after_dir1 >= 2,
            "Should restart on change in first watch dir"
        );

        // Change file in second directory
        tokio::fs::write(watch_dir2.join("file2.txt"), "content2")
            .await
            .expect("Failed to write to dir2");

        tokio::time::sleep(Duration::from_secs(2)).await;

        let final_content = tokio::fs::read_to_string(&counter_file)
            .await
            .expect("Failed to read counter");
        let final_count = final_content
            .lines()
            .filter(|l| l.contains("started"))
            .count();

        assert!(
            final_count > count_after_dir1,
            "Should also restart on change in second watch dir, got {} starts (was {})",
            final_count,
            count_after_dir1
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

        tokio::time::sleep(Duration::from_millis(300)).await;

        let initial_content = tokio::fs::read_to_string(&counter_file)
            .await
            .expect("Failed to read counter");
        let initial_count = initial_content
            .lines()
            .filter(|l| l.contains("started"))
            .count();

        // Create a file - should NOT trigger restart (no watch configured)
        tokio::fs::write(&some_file, "content")
            .await
            .expect("Failed to write file");

        tokio::time::sleep(Duration::from_secs(1)).await;

        let final_content = tokio::fs::read_to_string(&counter_file)
            .await
            .expect("Failed to read counter");
        let final_count = final_content
            .lines()
            .filter(|l| l.contains("started"))
            .count();

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

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Make many rapid changes
        for i in 0..10 {
            tokio::fs::write(&watch_file, format!("change {}", i))
                .await
                .expect("Failed to write");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // Wait for restarts to settle
        tokio::time::sleep(Duration::from_secs(3)).await;

        let content = tokio::fs::read_to_string(&counter_file)
            .await
            .expect("Failed to read counter");
        let restart_count = content.lines().filter(|l| l.contains("started")).count();

        // Should have restarted, but not 10 times (due to debouncing)
        assert!(
            restart_count >= 2,
            "Should restart at least once after file changes"
        );
        assert!(
            restart_count < 10,
            "Rapid changes should be debounced, got {} restarts",
            restart_count
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}
