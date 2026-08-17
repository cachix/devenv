//! Process lifecycle integration tests for NativeProcessManager.
//!
//! Note: Some tests use watchexec-supervisor directly because the manager
//! currently hardcodes `/bin/bash` which doesn't exist on NixOS.
//! See TODO: fix manager to use `bash` from PATH.

mod common;

use common::*;
use devenv_processes::{
    ProcessConfig, ProcessManager, ProcessPhase, RestartConfig, RestartPolicy, ShutdownConfig,
};
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

/// Test that an OS-level spawn failure is returned by start_command.
#[tokio::test(flavor = "multi_thread")]
async fn test_start_reports_spawn_failure() {
    let ctx = TestContext::new();
    let missing_cwd = ctx.temp_path().join("missing-working-directory");
    let config = ProcessConfig {
        name: "spawn-failure".to_string(),
        exec: "sleep 3600".to_string(),
        cwd: Some(missing_cwd),
        ..Default::default()
    };
    let manager = ctx.create_manager();

    let error = match manager.start_command(&config, None).await {
        Ok(_) => panic!("starting with a nonexistent working directory should fail"),
        Err(error) => error,
    };

    let message = error.to_string();
    assert!(
        message.contains("Failed to spawn process 'spawn-failure'"),
        "unexpected error: {message}"
    );
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
        let ctx = TestContext::new();
        let ready_file = ctx.temp_path().join("ready.txt");

        // Script that installs a SIGTERM-ignore trap, signals ready, then sleeps forever
        let script = format!(
            r#"trap '' TERM; echo ready > {}; sleep 3600"#,
            ready_file.display()
        );
        let job = run_shell_command(&script).await;

        // Wait for the trap to be installed (signaled by the ready file)
        assert!(
            wait_for_file(&ready_file, Duration::from_secs(5)).await,
            "Script should signal ready after installing trap"
        );

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

/// Test that manager.stop() waits for TERM trap cleanup before returning.
#[tokio::test(flavor = "multi_thread")]
async fn test_manager_stop_waits_for_term_cleanup() {
    timeout(Duration::from_secs(15), async {
        let ctx = TestContext::new();
        let ready_file = ctx.temp_path().join("ready.txt");
        let cleanup_file = ctx.temp_path().join("cleanup.txt");

        let script = ctx
            .create_script(
                "term-cleanup.sh",
                r#"#!/bin/sh
cleanup_file="$1"
ready_file="$2"

trap 'sleep 0.2; echo stopped > "$cleanup_file"; exit 0' TERM

echo ready > "$ready_file"
while true; do
  sleep 1
done
"#,
            )
            .await;

        let manager = ctx.create_manager();
        let config = ProcessConfig {
            name: "term-cleanup".to_string(),
            exec: format!(
                "{} {} {}",
                script.display(),
                cleanup_file.display(),
                ready_file.display()
            ),
            args: vec![],
            ..Default::default()
        };

        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        assert!(
            wait_for_file(&ready_file, Duration::from_secs(5)).await,
            "Script should signal ready"
        );

        manager.stop("term-cleanup").await.expect("Failed to stop");

        assert!(
            cleanup_file.exists(),
            "TERM cleanup should finish before stop() returns"
        );
        let cleanup = tokio::fs::read_to_string(&cleanup_file).await.unwrap();
        assert!(cleanup.contains("stopped"));
    })
    .await
    .expect("Test timed out");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn test_stop_reports_stopping_and_signals_leader_once() {
    timeout(Duration::from_secs(15), async {
        let ctx = TestContext::new();
        let ready_file = ctx.temp_path().join("single-signal.ready");
        let signal_file = ctx.temp_path().join("single-signal.count");
        let script = ctx
            .create_script(
                "single-signal.sh",
                r#"#!/bin/sh
signal_file="$1"
ready_file="$2"

trap 'echo term >> "$signal_file"; sleep 0.3; exit 0' TERM
touch "$ready_file"
while :; do
  sleep 1
done
"#,
            )
            .await;

        let manager = Arc::new(ctx.create_manager());
        let config = ProcessConfig {
            name: "single-signal".to_string(),
            exec: format!(
                "{} {} {}",
                script.display(),
                signal_file.display(),
                ready_file.display()
            ),
            shutdown: ShutdownConfig {
                signal: 15,
                grace: 2,
            },
            ..Default::default()
        };
        manager.start_command(&config, None).await.unwrap();
        assert!(wait_for_file(&ready_file, STARTUP_TIMEOUT).await);

        let stopping_manager = Arc::clone(&manager);
        let stop = tokio::spawn(async move { stopping_manager.stop("single-signal").await });
        assert!(wait_for_file(&signal_file, STARTUP_TIMEOUT).await);
        assert_eq!(
            manager.get_phase("single-signal").await,
            Some(ProcessPhase::Stopping),
            "the process must remain Stopping until its TERM cleanup completes"
        );

        manager
            .stop_all()
            .await
            .expect("stop_all must wait for the in-flight stop");
        assert_eq!(
            manager.get_phase("single-signal").await,
            Some(ProcessPhase::Stopped)
        );
        stop.await.unwrap().unwrap();
        let signals = tokio::fs::read_to_string(&signal_file).await.unwrap();
        assert_eq!(
            signals.lines().count(),
            1,
            "the leader process group received the graceful signal more than once"
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

/// Test that a process reading stdin gets EOF immediately and exits cleanly
#[tokio::test(flavor = "multi_thread")]
async fn test_stdin_closed_for_processes() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let manager = ctx.create_manager();

        // Script reads from stdin, then writes what it got. With stdin closed
        // (/dev/null), `read` returns immediately with empty input.
        let config = ProcessConfig {
            name: "stdin-reader".to_string(),
            exec: "bash -c 'read line; echo \"got: [$line]\"'".to_string(),
            ..Default::default()
        };

        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // The process should complete and write output since stdin is /dev/null.
        // `read` gets EOF immediately so the script finishes without hanging.
        let stdout_log = ctx.state_dir.join("logs/stdin-reader.stdout.log");
        assert!(
            wait_for_file_content(&stdout_log, "got: []", STARTUP_TIMEOUT).await,
            "Process should have received empty stdin and written output"
        );

        manager.stop_all().await.expect("Failed to stop all");
    })
    .await
    .expect("Test timed out");
}

/// Test that stopping a process also kills its child processes.
///
/// Simulates a service (like postgres) that spawns a child worker. When the
/// parent is stopped, the child must also be terminated.
#[tokio::test(flavor = "multi_thread")]
async fn test_stop_kills_child_processes() {
    timeout(Duration::from_secs(15), async {
        let ctx = TestContext::new();
        let child_pid_file = ctx.temp_path().join("child.pid");
        let ready_file = ctx.temp_path().join("ready.txt");

        let config = ProcessConfig {
            name: "parent-with-child".to_string(),
            // Spawn a background child process and write its PID to a file
            exec: format!(
                r#"bash -c 'sleep 3600 &
echo $! > {}
echo ready > {}
wait'"#,
                child_pid_file.display(),
                ready_file.display()
            ),
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        assert!(
            wait_for_file(&ready_file, STARTUP_TIMEOUT).await,
            "Process should signal ready"
        );

        // Read the child PID
        let child_pid_str = tokio::fs::read_to_string(&child_pid_file)
            .await
            .expect("Failed to read child PID");
        let child_pid: i32 = child_pid_str.trim().parse().expect("Invalid child PID");

        // Verify child is running
        assert_eq!(
            unsafe { nix::libc::kill(child_pid, 0) },
            0,
            "Child process should be running before stop"
        );

        // Stop the parent
        manager
            .stop("parent-with-child")
            .await
            .expect("Failed to stop");

        // Wait briefly for signals to propagate
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify child is also gone
        assert_ne!(
            unsafe { nix::libc::kill(child_pid, 0) },
            0,
            "Child process should be killed after parent is stopped"
        );
    })
    .await
    .expect("Test timed out");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_shutdown_during_restart_does_not_start_again() {
    timeout(Duration::from_secs(15), async {
        let ctx = TestContext::new();
        let starts_file = ctx.temp_path().join("starts.txt");
        let stopping_file = ctx.temp_path().join("stopping.txt");
        let script = ctx
            .create_script(
                "slow-stop.sh",
                r#"#!/bin/sh
starts_file="$1"
stopping_file="$2"

echo started >> "$starts_file"
trap 'touch "$stopping_file"; sleep 0.5; exit 0' TERM

while true; do
  sleep 1
done
"#,
            )
            .await;

        let manager = Arc::new(ctx.create_manager());
        let config = ProcessConfig {
            name: "restart-during-shutdown".to_string(),
            exec: format!(
                "{} {} {}",
                script.display(),
                starts_file.display(),
                stopping_file.display()
            ),
            shutdown: ShutdownConfig {
                signal: 15,
                grace: 2,
            },
            ..Default::default()
        };
        manager
            .start_command(&config, None)
            .await
            .expect("start process");
        assert!(wait_for_file(&starts_file, STARTUP_TIMEOUT).await);

        let restart_manager = Arc::clone(&manager);
        let restart =
            tokio::spawn(async move { restart_manager.restart("restart-during-shutdown").await });
        assert!(
            wait_for_file(&stopping_file, STARTUP_TIMEOUT).await,
            "restart did not begin stopping the process"
        );

        manager.stop_all().await.expect("stop manager");
        let restart_error = restart
            .await
            .expect("restart task")
            .expect_err("restart should stop when manager shutdown begins");
        assert!(format!("{restart_error:?}").contains("shutting down"));

        let starts = tokio::fs::read_to_string(&starts_file)
            .await
            .expect("read starts file");
        assert_eq!(
            starts.lines().count(),
            1,
            "process started again after shutdown began"
        );
    })
    .await
    .expect("Test timed out");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn test_stop_during_exit_cleanup_prevents_automatic_restart() {
    timeout(Duration::from_secs(15), async {
        let ctx = TestContext::new();
        let starts_file = ctx.temp_path().join("auto-restart.starts");
        let child_ready = ctx.temp_path().join("auto-restart.child-ready");
        let cleanup_started = ctx.temp_path().join("auto-restart.cleanup-started");
        let script = ctx
            .create_script(
                "crash-with-stubborn-child.sh",
                r#"#!/bin/bash
starts_file="$1"
child_ready="$2"
cleanup_started="$3"

echo started >> "$starts_file"
set -m
stubborn_child() {
  trap 'touch "$cleanup_started"' TERM
  touch "$child_ready"
  while :; do sleep 1; done
}
stubborn_child &
while [ ! -e "$child_ready" ]; do sleep 0.01; done
exit 1
"#,
            )
            .await;

        let manager = Arc::new(ctx.create_manager());
        let config = ProcessConfig {
            name: "stop-during-exit-cleanup".to_string(),
            exec: format!(
                "{} {} {} {}",
                script.display(),
                starts_file.display(),
                child_ready.display(),
                cleanup_started.display()
            ),
            restart: RestartConfig {
                on: RestartPolicy::Always,
                ..Default::default()
            },
            shutdown: ShutdownConfig {
                signal: 15,
                grace: 2,
            },
            ..Default::default()
        };
        manager.start_command(&config, None).await.unwrap();
        assert!(
            wait_for_file(&cleanup_started, STARTUP_TIMEOUT).await,
            "supervisor did not begin cleaning the crashed process session"
        );

        manager.stop("stop-during-exit-cleanup").await.unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;

        let starts = tokio::fs::read_to_string(&starts_file).await.unwrap();
        assert_eq!(
            starts.lines().count(),
            1,
            "the process restarted after stop was requested during exit cleanup"
        );
        assert_eq!(
            manager.get_phase("stop-during-exit-cleanup").await,
            Some(ProcessPhase::Stopped)
        );
    })
    .await
    .expect("Test timed out");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn test_stop_kills_private_process_groups_in_service_session() {
    timeout(Duration::from_secs(15), async {
        let ctx = TestContext::new();
        let child_pid_file = ctx.temp_path().join("private-child.pid");
        let ready_file = ctx.temp_path().join("private-child.ready");

        let config = ProcessConfig {
            name: "parent-with-private-group".to_string(),
            // Bash monitor mode puts an asynchronous command in a distinct
            // process group while retaining the service's session.
            exec: format!(
                r#"bash -c 'set -m
sleep 3600 &
echo $! > {}
echo ready > {}
wait'"#,
                child_pid_file.display(),
                ready_file.display()
            ),
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        assert!(
            wait_for_file(&ready_file, STARTUP_TIMEOUT).await,
            "Process should signal ready"
        );

        let child_pid: i32 = tokio::fs::read_to_string(&child_pid_file)
            .await
            .expect("Failed to read child PID")
            .trim()
            .parse()
            .expect("Invalid child PID");

        manager
            .stop("parent-with-private-group")
            .await
            .expect("Failed to stop");

        let child_exited = wait_for_condition(
            || async { (unsafe { nix::libc::kill(child_pid, 0) }) != 0 },
            Duration::from_secs(2),
        )
        .await;

        if !child_exited {
            // Do not leave the test process running after a failure.
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(child_pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }

        assert!(
            child_exited,
            "A descendant in a private process group survived service shutdown"
        );
    })
    .await
    .expect("Test timed out");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn test_stop_uses_configured_signal_for_private_process_groups() {
    timeout(Duration::from_secs(15), async {
        let ctx = TestContext::new();
        let marker = ctx.temp_path().join("private-child.interrupted");
        let ready = ctx.temp_path().join("private-child.ready");

        let config = ProcessConfig {
            name: "configured-private-group-signal".to_string(),
            exec: format!(
                r#"bash -c 'set -m
bash -c '\''trap "echo interrupted > {}; exit 0" INT; touch {}; while :; do sleep 1; done'\'' &
wait'"#,
                marker.display(),
                ready.display(),
            ),
            shutdown: ShutdownConfig {
                signal: 2,
                grace: 2,
            },
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");
        assert!(
            wait_for_file(&ready, STARTUP_TIMEOUT).await,
            "private process group did not become ready"
        );

        manager
            .stop("configured-private-group-signal")
            .await
            .expect("Failed to stop");

        assert!(
            wait_for_file(&marker, Duration::from_secs(2)).await,
            "private process group did not receive configured SIGINT"
        );
    })
    .await
    .expect("Test timed out");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn test_shutdown_grace_is_shared_by_all_session_groups() {
    timeout(Duration::from_secs(12), async {
        let ctx = TestContext::new();
        let ready = ctx.temp_path().join("shared-grace.ready");

        let config = ProcessConfig {
            name: "shared-shutdown-grace".to_string(),
            exec: format!(
                r#"bash -c 'set -m
trap "" TERM
bash -c '\''trap "" TERM; touch {}; while :; do sleep 1; done'\'' &
wait'"#,
                ready.display(),
            ),
            shutdown: ShutdownConfig {
                signal: 15,
                grace: 2,
            },
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");
        assert!(
            wait_for_file(&ready, STARTUP_TIMEOUT).await,
            "private process group did not become ready"
        );

        let started = std::time::Instant::now();
        manager
            .stop("shared-shutdown-grace")
            .await
            .expect("Failed to stop");
        let elapsed = started.elapsed();

        assert!(
            elapsed >= Duration::from_millis(1500),
            "test processes did not exercise the grace period: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(3500),
            "shutdown applied the grace period more than once: {elapsed:?}"
        );
    })
    .await
    .expect("Test timed out");
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
