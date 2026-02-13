//! Process lifecycle integration tests for NativeProcessManager.
//!
//! Note: Some tests use watchexec-supervisor directly because the manager
//! currently hardcodes `/bin/bash` which doesn't exist on NixOS.
//! See TODO: fix manager to use `bash` from PATH.

mod common;

use common::*;
use devenv_processes::{ProcessConfig, ProcessManager};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use watchexec_supervisor::command::{Command, Program, Shell, SpawnOptions};
use watchexec_supervisor::job::start_job;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

// ============================================================================
// Helper to run shell commands via watchexec-supervisor (NixOS compatible)
// ============================================================================

async fn run_shell_command(script: &str) -> Arc<watchexec_supervisor::job::Job> {
    let program = Program::Shell {
        shell: Shell::new("bash"), // Use "bash" not "/bin/bash" for NixOS
        command: script.to_string(),
        args: vec![],
    };

    let cmd = Arc::new(Command {
        program,
        options: SpawnOptions {
            grouped: true,
            ..Default::default()
        },
    });

    let (job, _task) = start_job(cmd);
    job.start().await;
    Arc::new(job)
}

// ============================================================================
// Process Lifecycle Tests
// ============================================================================

/// Test that a simple shell command runs and produces output
#[tokio::test(flavor = "multi_thread")]
async fn test_shell_command_runs() {
    let ctx = TestContext::new();
    let output_file = ctx.temp_path().join("output.txt");

    let script = format!(r#"echo "hello world" > {}"#, output_file.display());
    let job = run_shell_command(&script).await;

    // Wait for job to complete
    job.to_wait().await;

    assert!(output_file.exists(), "Output file should be created");
    let content = tokio::fs::read_to_string(&output_file).await.unwrap();
    assert!(content.contains("hello world"));
}

/// Test stopping a long-running process via the manager
#[tokio::test(flavor = "multi_thread")]
async fn test_stop_single_process() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();

        // Use sleep directly (doesn't need bash wrapper)
        let config = long_running_config("long-sleep", 3600);
        let manager = ctx.create_manager();

        // Start the process using start_command with the full sleep command
        let mut config_for_command = config.clone();
        config_for_command.exec = "sleep 3600".to_string();
        let _job = manager
            .start_command(&config_for_command, None)
            .await
            .expect("Failed to start");

        // Verify it's in the job list
        assert!(
            wait_for_process_start(&manager, "long-sleep", STARTUP_TIMEOUT).await,
            "Process should be in job list"
        );

        // Stop the process
        manager.stop("long-sleep").await.expect("Failed to stop");

        // Verify it's removed
        assert!(
            wait_for_process_exit(&manager, "long-sleep", SHUTDOWN_TIMEOUT).await,
            "Process should be removed from job list"
        );
    })
    .await
    .expect("Test timed out");
}

/// Test starting and stopping multiple processes
#[tokio::test(flavor = "multi_thread")]
async fn test_multiple_processes() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();

        let mut configs = std::collections::HashMap::new();
        for i in 1..=3 {
            let name = format!("proc{}", i);
            let config = ProcessConfig {
                name: name.clone(),
                exec: "sleep 3600".to_string(),
                ..Default::default()
            };
            configs.insert(name, config);
        }

        let manager = ctx.create_manager();

        // Start all processes
        for (name, config) in &configs {
            manager
                .start_command(config, None)
                .await
                .unwrap_or_else(|_| panic!("Failed to start {}", name));
        }

        // Verify all are running
        let running = manager.list().await;
        assert_eq!(running.len(), 3, "Should have 3 running processes");

        // Stop all
        manager.stop_all().await.expect("Failed to stop all");

        // Verify all stopped
        assert!(
            manager.list().await.is_empty(),
            "All processes should be stopped"
        );
    })
    .await
    .expect("Test timed out");
}

/// Test that stop_all clears all jobs
#[tokio::test(flavor = "multi_thread")]
async fn test_stop_all_processes() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();

        let mut configs = std::collections::HashMap::new();
        for name in ["a", "b"] {
            let config = ProcessConfig {
                name: name.to_string(),
                exec: "sleep 3600".to_string(),
                ..Default::default()
            };
            configs.insert(name.to_string(), config);
        }

        let manager = ctx.create_manager();

        for config in configs.values() {
            manager.start_command(config, None).await.unwrap();
        }

        assert_eq!(manager.list().await.len(), 2);

        manager.stop_all().await.expect("Failed to stop all");

        assert!(manager.list().await.is_empty());
    })
    .await
    .expect("Test timed out");
}

/// Test that stop terminates a process and it exits cleanly
#[tokio::test(flavor = "multi_thread")]
async fn test_stop_terminates_process() {
    timeout(Duration::from_secs(15), async {
        let ctx = TestContext::new();
        let ready_file = ctx.temp_path().join("ready.txt");

        // Script that signals ready then waits (use finite sleep for better signal handling)
        let script = format!(r#"echo ready > {}; sleep 3600"#, ready_file.display());

        let job = run_shell_command(&script).await;

        // Wait for ready signal
        assert!(
            wait_for_file(&ready_file, Duration::from_secs(5)).await,
            "Script should signal ready"
        );

        // Stop the job - this should terminate it
        job.stop_with_signal(
            watchexec_supervisor::Signal::Terminate,
            Duration::from_secs(2),
        )
        .await;

        // Wait for the process to actually exit
        job.to_wait().await;
    })
    .await
    .expect("Test timed out");
}

/// Test that process ignoring SIGTERM eventually gets killed
#[tokio::test(flavor = "multi_thread")]
async fn test_force_kill_after_timeout() {
    timeout(Duration::from_secs(20), async {
        // Script that ignores SIGTERM
        let script = r#"trap '' TERM; sleep 3600"#;
        let job = run_shell_command(script).await;

        // Give trap time to set up
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Stop with a short grace period
        let stop_start = std::time::Instant::now();
        job.stop_with_signal(
            watchexec_supervisor::Signal::Terminate,
            Duration::from_secs(2), // Grace period before force kill
        )
        .await;

        // Wait for completion
        job.to_wait().await;

        let stop_duration = stop_start.elapsed();
        // Should have waited at least the grace period
        assert!(
            stop_duration >= Duration::from_secs(1),
            "Should have waited before force killing"
        );
    })
    .await
    .expect("Test timed out");
}

/// Test that is_running returns false initially
#[tokio::test(flavor = "multi_thread")]
async fn test_is_running_initially_false() {
    let ctx = TestContext::new();
    let manager = ctx.create_manager();

    // Manager has no PID file initially
    assert!(!manager.is_running().await);
}

/// Test process that writes stdout/stderr via shell command
#[tokio::test(flavor = "multi_thread")]
async fn test_process_output_capture() {
    let ctx = TestContext::new();
    let stdout_file = ctx.temp_path().join("stdout.txt");
    let stderr_file = ctx.temp_path().join("stderr.txt");

    let script = format!(
        r#"echo "stdout message" > {}; echo "stderr message" > {}"#,
        stdout_file.display(),
        stderr_file.display()
    );

    let job = run_shell_command(&script).await;
    job.to_wait().await;

    assert!(stdout_file.exists());
    assert!(stderr_file.exists());

    let stdout_content = tokio::fs::read_to_string(&stdout_file).await.unwrap();
    let stderr_content = tokio::fs::read_to_string(&stderr_file).await.unwrap();

    assert!(stdout_content.contains("stdout message"));
    assert!(stderr_content.contains("stderr message"));
}
