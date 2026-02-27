//! Readiness probe integration tests for NativeProcessManager.

mod common;

use common::*;
use devenv_processes::{
    HttpGetProbe as HttpGetProbeConfig, HttpProbe as HttpProbeConfig, ProcessConfig, ReadyConfig,
    RestartConfig, RestartPolicy,
};
use std::time::Duration;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// wait_ready returns an error when the cancellation token is triggered,
/// instead of blocking indefinitely on a process that never becomes ready.
#[tokio::test(flavor = "multi_thread")]
async fn test_wait_ready_returns_on_cancellation() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let manager = ctx.create_manager();

        // Process that runs forever and never signals readiness.
        // Give it a ready config so the supervisor does NOT mark it
        // immediately ready (exec probe with impossible command).
        let config = ProcessConfig {
            name: "never-ready".to_string(),
            exec: "sleep 3600".to_string(),
            ready: Some(ReadyConfig {
                exec: Some("exit 1".to_string()),
                ..Default::default()
            }),
            restart: RestartConfig {
                on: RestartPolicy::Never,
                ..Default::default()
            },
            ..Default::default()
        };

        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        assert!(
            wait_for_process_start(&manager, "never-ready", STARTUP_TIMEOUT).await,
            "Process should be in job list"
        );

        let cancel = CancellationToken::new();
        cancel.cancel();

        let result = manager.wait_ready("never-ready", &cancel).await;
        assert!(
            result.is_err(),
            "wait_ready should return error when cancelled"
        );

        manager.stop_all().await.expect("Failed to stop all");
    })
    .await
    .expect("Test timed out");
}

/// A process with an exec readiness probe transitions to Ready after the
/// probe command succeeds.
#[tokio::test(flavor = "multi_thread")]
async fn test_exec_probe_readiness() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let manager = ctx.create_manager();

        let marker = ctx.temp_path().join("ready-marker");

        // Process creates the marker file after a short delay then sleeps.
        let exec_cmd = format!(
            "sh -c 'sleep 0.2 && touch {} && sleep 3600'",
            marker.display()
        );

        let config = ProcessConfig {
            name: "exec-ready".to_string(),
            exec: exec_cmd,
            ready: Some(ReadyConfig {
                exec: Some(format!("test -f {}", marker.display())),
                period: 1,
                probe_timeout: 5,
                ..Default::default()
            }),
            restart: RestartConfig {
                on: RestartPolicy::Never,
                ..Default::default()
            },
            ..Default::default()
        };

        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        assert!(
            wait_for_process_start(&manager, "exec-ready", STARTUP_TIMEOUT).await,
            "Process should be in job list"
        );

        let cancel = CancellationToken::new();
        let result = manager.wait_ready("exec-ready", &cancel).await;
        assert!(
            result.is_ok(),
            "wait_ready should succeed once exec probe passes: {:?}",
            result.err()
        );

        manager.stop_all().await.expect("Failed to stop all");
    })
    .await
    .expect("Test timed out");
}

/// A process with an HTTP readiness probe transitions to Ready after the
/// probe receives a 2xx response.
#[tokio::test(flavor = "multi_thread")]
async fn test_http_probe_readiness() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let manager = ctx.create_manager();

        // Bind a test HTTP server that always responds 200.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind test HTTP server");
        let port = listener.local_addr().unwrap().port();

        let _server = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = [0u8; 1024];
                let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;
                let response = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
                let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
            }
        });

        // The process itself is just sleep; the HTTP probe points at our test server.
        let config = ProcessConfig {
            name: "http-ready".to_string(),
            exec: "sleep 3600".to_string(),
            ready: Some(ReadyConfig {
                http: Some(HttpProbeConfig {
                    get: Some(HttpGetProbeConfig {
                        host: "127.0.0.1".to_string(),
                        port,
                        path: "/".to_string(),
                        scheme: "http".to_string(),
                    }),
                }),
                period: 1,
                probe_timeout: 5,
                ..Default::default()
            }),
            restart: RestartConfig {
                on: RestartPolicy::Never,
                ..Default::default()
            },
            ..Default::default()
        };

        manager
            .start_command(&config, None)
            .await
            .expect("Failed to start");

        assert!(
            wait_for_process_start(&manager, "http-ready", STARTUP_TIMEOUT).await,
            "Process should be in job list"
        );

        let cancel = CancellationToken::new();
        let result = manager.wait_ready("http-ready", &cancel).await;
        assert!(
            result.is_ok(),
            "wait_ready should succeed once HTTP probe passes: {:?}",
            result.err()
        );

        manager.stop_all().await.expect("Failed to stop all");
    })
    .await
    .expect("Test timed out");
}
