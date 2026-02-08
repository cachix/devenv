//! Socket activation integration tests for NativeProcessManager.
//!
//! Tests TCP and Unix socket activation using the systemd-style
//! LISTEN_FDS environment variable mechanism.

mod common;

use common::*;
use devenv_processes::ProcessConfig;
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

// ============================================================================
// TCP Socket Activation Tests
// ============================================================================

/// Test that TCP socket activation sets LISTEN_FDS environment variable
#[tokio::test(flavor = "multi_thread")]
async fn test_tcp_socket_activation_env_vars() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let output_file = ctx.temp_path().join("listen_env.txt");

        // Create a script that writes LISTEN_FDS to a file
        let script = ctx
            .create_script("check_env.sh", &script_echo_listen_fds(&output_file))
            .await;

        // Use port 0 to let OS assign a free port
        let config = tcp_socket_config("tcp-env-test", &script, "127.0.0.1:0");
        let manager = ctx.create_manager_single(config.clone());

        // Start the process
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for the output file to be created
        assert!(
            wait_for_file(&output_file, STARTUP_TIMEOUT).await,
            "Output file should be created"
        );

        // Read and verify the environment variables
        let content = tokio::fs::read_to_string(&output_file)
            .await
            .expect("Failed to read output");

        assert!(
            content.contains("LISTEN_FDS=1"),
            "LISTEN_FDS should be set to 1, got: {}",
            content
        );
        assert!(
            content.contains("LISTEN_PID="),
            "LISTEN_PID should be set, got: {}",
            content
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

/// Test that TCP socket is accessible before process starts (socket activation)
#[tokio::test(flavor = "multi_thread")]
async fn test_tcp_socket_available_immediately() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();

        // Find a free port
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener); // Release the port

        let output_file = ctx.temp_path().join("started.txt");

        // Script that writes to file then sleeps (to keep socket open)
        let script_content = format!(
            r#"#!/bin/sh
echo "started" > {}
sleep 3600
"#,
            output_file.display()
        );
        let script = ctx.create_script("server.sh", &script_content).await;

        let config = tcp_socket_config("tcp-immediate", &script, &addr.to_string());
        let manager = ctx.create_manager_single(config.clone());

        // Start the process
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // The socket should be listening immediately (created by manager before process starts)
        // Wait for process to start first
        assert!(
            wait_for_file(&output_file, STARTUP_TIMEOUT).await,
            "Process should start"
        );

        // Now verify the socket is accessible
        assert!(
            wait_for_tcp_port(&addr.to_string(), SOCKET_TIMEOUT).await,
            "TCP socket should be accessible at {}",
            addr
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

// ============================================================================
// Unix Socket Activation Tests
// ============================================================================

/// Test that Unix socket activation sets LISTEN_FDS environment variable
#[tokio::test(flavor = "multi_thread")]
async fn test_unix_socket_activation_env_vars() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let output_file = ctx.temp_path().join("listen_env.txt");
        let socket_path = ctx.temp_path().join("test.sock");

        // Create a script that writes LISTEN_FDS to a file
        let script = ctx
            .create_script("check_env.sh", &script_echo_listen_fds(&output_file))
            .await;

        let config = unix_socket_config("unix-env-test", &script, socket_path);
        let manager = ctx.create_manager_single(config.clone());

        // Start the process
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for the output file to be created
        assert!(
            wait_for_file(&output_file, STARTUP_TIMEOUT).await,
            "Output file should be created"
        );

        // Read and verify the environment variables
        let content = tokio::fs::read_to_string(&output_file)
            .await
            .expect("Failed to read output");

        assert!(
            content.contains("LISTEN_FDS=1"),
            "LISTEN_FDS should be set to 1, got: {}",
            content
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

/// Test that Unix socket file is created with correct permissions
#[tokio::test(flavor = "multi_thread")]
async fn test_unix_socket_permissions() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let socket_path = ctx.temp_path().join("perms.sock");
        let output_file = ctx.temp_path().join("started.txt");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" > {}
sleep 3600
"#,
            output_file.display()
        );
        let script = ctx.create_script("server.sh", &script_content).await;

        // Config with mode 0o600
        let config = unix_socket_config("unix-perms", &script, socket_path.clone());
        let manager = ctx.create_manager_single(config.clone());

        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for process to start
        assert!(
            wait_for_file(&output_file, STARTUP_TIMEOUT).await,
            "Process should start"
        );

        // Check socket file exists
        assert!(socket_path.exists(), "Unix socket file should exist");

        // Check permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&socket_path).expect("Failed to get metadata");
            let mode = metadata.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "Socket should have mode 0600, got {:o}", mode);
        }

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

/// Test that Unix socket is accessible
#[tokio::test(flavor = "multi_thread")]
async fn test_unix_socket_connectable() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let socket_path = ctx.temp_path().join("connect.sock");
        let output_file = ctx.temp_path().join("started.txt");

        let script_content = format!(
            r#"#!/bin/sh
echo "started" > {}
sleep 3600
"#,
            output_file.display()
        );
        let script = ctx.create_script("server.sh", &script_content).await;

        let config = unix_socket_config("unix-connect", &script, socket_path.clone());
        let manager = ctx.create_manager_single(config.clone());

        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for process to start
        assert!(
            wait_for_file(&output_file, STARTUP_TIMEOUT).await,
            "Process should start"
        );

        // Verify we can connect to the Unix socket
        assert!(
            wait_for_unix_socket(&socket_path, SOCKET_TIMEOUT).await,
            "Should be able to connect to Unix socket"
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

// ============================================================================
// Multiple Sockets Tests
// ============================================================================

/// Test process with multiple listen specifications
#[tokio::test(flavor = "multi_thread")]
async fn test_multiple_listen_fds() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let output_file = ctx.temp_path().join("multi_listen.txt");

        // Create script that logs LISTEN_FDS
        let script = ctx
            .create_script("check_env.sh", &script_echo_listen_fds(&output_file))
            .await;

        // Create config with two TCP sockets
        use devenv_processes::{ListenKind, ListenSpec};
        let config = ProcessConfig {
            name: "multi-socket".to_string(),
            exec: script.to_string_lossy().to_string(),
            listen: vec![
                ListenSpec {
                    name: "http".to_string(),
                    kind: ListenKind::Tcp,
                    address: Some("127.0.0.1:0".to_string()),
                    path: None,
                    backlog: Some(128),
                    mode: None,
                },
                ListenSpec {
                    name: "https".to_string(),
                    kind: ListenKind::Tcp,
                    address: Some("127.0.0.1:0".to_string()),
                    path: None,
                    backlog: Some(128),
                    mode: None,
                },
            ],
            ..Default::default()
        };

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        // Wait for output
        assert!(
            wait_for_file(&output_file, STARTUP_TIMEOUT).await,
            "Output file should be created"
        );

        let content = tokio::fs::read_to_string(&output_file)
            .await
            .expect("Failed to read");

        // Should have 2 FDs
        assert!(
            content.contains("LISTEN_FDS=2"),
            "LISTEN_FDS should be 2 for two sockets, got: {}",
            content
        );

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}

/// Test mixed TCP and Unix socket activation
#[tokio::test(flavor = "multi_thread")]
async fn test_mixed_tcp_unix_sockets() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let output_file = ctx.temp_path().join("mixed_listen.txt");
        let socket_path = ctx.temp_path().join("mixed.sock");

        let script = ctx
            .create_script("check_env.sh", &script_echo_listen_fds(&output_file))
            .await;

        use devenv_processes::{ListenKind, ListenSpec};
        let config = ProcessConfig {
            name: "mixed-socket".to_string(),
            exec: script.to_string_lossy().to_string(),
            listen: vec![
                ListenSpec {
                    name: "tcp".to_string(),
                    kind: ListenKind::Tcp,
                    address: Some("127.0.0.1:0".to_string()),
                    path: None,
                    backlog: Some(128),
                    mode: None,
                },
                ListenSpec {
                    name: "unix".to_string(),
                    kind: ListenKind::UnixStream,
                    address: None,
                    path: Some(socket_path.clone()),
                    backlog: Some(128),
                    mode: Some(0o666),
                },
            ],
            ..Default::default()
        };

        let manager = ctx.create_manager_single(config.clone());
        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        assert!(
            wait_for_file(&output_file, STARTUP_TIMEOUT).await,
            "Output file should be created"
        );

        let content = tokio::fs::read_to_string(&output_file)
            .await
            .expect("Failed to read");

        assert!(
            content.contains("LISTEN_FDS=2"),
            "Should have 2 FDs (TCP + Unix), got: {}",
            content
        );

        // Both sockets should exist
        assert!(socket_path.exists(), "Unix socket should exist");

        manager.stop_all().await.expect("Failed to stop");
    })
    .await
    .expect("Test timed out");
}
