//! Shared test utilities for devenv-processes integration tests.

// Each test file compiles separately, so not all helpers are used in each binary
#![allow(dead_code)]

use devenv_processes::{
    ListenKind, ListenSpec, NativeProcessManager, ProcessConfig, RestartPolicy, WatchConfig,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::fs;
use tokio::task::JoinHandle;

/// Test context that manages temp directories and cleanup
pub struct TestContext {
    pub temp_dir: TempDir,
    pub state_dir: PathBuf,
}

impl TestContext {
    pub fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let state_dir = temp_dir.path().join("state");
        std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");
        Self {
            temp_dir,
            state_dir,
        }
    }

    pub fn temp_path(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Create an executable script in the temp directory
    pub async fn create_script(&self, name: &str, content: &str) -> PathBuf {
        let path = self.temp_dir.path().join(name);
        fs::write(&path, content)
            .await
            .expect("Failed to write script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .await
                .expect("Failed to set permissions");
        }
        path
    }

    /// Create a NativeProcessManager with the given process configs
    pub fn create_manager(&self, configs: HashMap<String, ProcessConfig>) -> NativeProcessManager {
        NativeProcessManager::new(self.state_dir.clone(), configs)
            .expect("Failed to create manager")
    }

    /// Create a NativeProcessManager with a single process config
    pub fn create_manager_single(&self, config: ProcessConfig) -> NativeProcessManager {
        let mut configs = HashMap::new();
        configs.insert(config.name.clone(), config);
        self.create_manager(configs)
    }
}

/// A manager with a background event loop for tests that need supervision.
pub struct ManagerWithEventLoop {
    pub manager: NativeProcessManager,
    pub cancel: tokio_util::sync::CancellationToken,
    event_loop_handle: JoinHandle<miette::Result<()>>,
}

impl ManagerWithEventLoop {
    /// Start the event loop in the background.
    /// Must be called AFTER all `start_command()` calls.
    pub async fn start(manager: NativeProcessManager) -> Self {
        let cancel = tokio_util::sync::CancellationToken::new();
        let event_loop_handle = manager.spawn_event_loop(cancel.clone()).await;
        Self {
            manager,
            cancel,
            event_loop_handle,
        }
    }

    /// Stop all processes and shut down the event loop.
    pub async fn shutdown(self) {
        self.manager.stop_all().await.ok();
        self.cancel.cancel();
        let _ = self.event_loop_handle.await;
    }
}

// ============================================================================
// Config Builders
// ============================================================================

/// Create a long-running process configuration (sleep)
pub fn long_running_config(name: &str, duration_secs: u32) -> ProcessConfig {
    ProcessConfig {
        name: name.to_string(),
        exec: "sleep".to_string(),
        args: vec![duration_secs.to_string()],
        restart: RestartPolicy::Never,
        ..Default::default()
    }
}

/// Create TCP socket activation config
pub fn tcp_socket_config(name: &str, script_path: &Path, address: &str) -> ProcessConfig {
    ProcessConfig {
        name: name.to_string(),
        exec: script_path.to_string_lossy().to_string(),
        args: vec![],
        listen: vec![ListenSpec {
            name: "http".to_string(),
            kind: ListenKind::Tcp,
            address: Some(address.to_string()),
            path: None,
            backlog: Some(128),
            mode: None,
        }],
        restart: RestartPolicy::Never,
        ..Default::default()
    }
}

/// Create Unix socket activation config
pub fn unix_socket_config(name: &str, script_path: &Path, socket_path: PathBuf) -> ProcessConfig {
    ProcessConfig {
        name: name.to_string(),
        exec: script_path.to_string_lossy().to_string(),
        args: vec![],
        listen: vec![ListenSpec {
            name: "socket".to_string(),
            kind: ListenKind::UnixStream,
            address: None,
            path: Some(socket_path),
            backlog: Some(128),
            mode: Some(0o600),
        }],
        restart: RestartPolicy::Never,
        ..Default::default()
    }
}

/// Create watch config
pub fn watch_process_config(
    name: &str,
    script_path: &Path,
    watch_paths: Vec<PathBuf>,
    ignore: Vec<String>,
) -> ProcessConfig {
    ProcessConfig {
        name: name.to_string(),
        exec: script_path.to_string_lossy().to_string(),
        args: vec![],
        watch: WatchConfig {
            paths: watch_paths,
            extensions: vec![],
            ignore,
        },
        restart: RestartPolicy::Never,
        ..Default::default()
    }
}

// ============================================================================
// Wait Helpers
// ============================================================================

/// Wait for a condition with exponential backoff
pub async fn wait_for_condition<F, Fut>(mut check: F, timeout: Duration) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + timeout;
    let mut delay = Duration::from_millis(10);
    let max_delay = Duration::from_millis(500);

    while Instant::now() < deadline {
        if check().await {
            return true;
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(max_delay);
    }
    false
}

/// Wait for a process to appear in the manager's job list
pub async fn wait_for_process_start(
    manager: &NativeProcessManager,
    name: &str,
    timeout: Duration,
) -> bool {
    wait_for_condition(
        || async { manager.list().await.contains(&name.to_string()) },
        timeout,
    )
    .await
}

/// Wait for a process to exit (no longer in job list)
pub async fn wait_for_process_exit(
    manager: &NativeProcessManager,
    name: &str,
    timeout: Duration,
) -> bool {
    wait_for_condition(
        || async { !manager.list().await.contains(&name.to_string()) },
        timeout,
    )
    .await
}

/// Wait for a file to exist
pub async fn wait_for_file(path: &Path, timeout: Duration) -> bool {
    let path = path.to_path_buf();
    wait_for_condition(|| async { path.exists() }, timeout).await
}

/// Wait for a file to contain expected content
pub async fn wait_for_file_content(path: &Path, expected: &str, timeout: Duration) -> bool {
    let path = path.to_path_buf();
    let expected = expected.to_string();
    wait_for_condition(
        || {
            let path = path.clone();
            let expected = expected.clone();
            async move {
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    content.contains(&expected)
                } else {
                    false
                }
            }
        },
        timeout,
    )
    .await
}

/// Wait for a file to have at least `expected` lines containing `pattern`.
/// Returns the actual count found (or the count at timeout).
pub async fn wait_for_line_count(
    path: &Path,
    pattern: &str,
    expected: usize,
    timeout: Duration,
) -> usize {
    let deadline = Instant::now() + timeout;
    let mut delay = Duration::from_millis(10);
    let max_delay = Duration::from_millis(500);
    let mut count = 0;

    while Instant::now() < deadline {
        if let Ok(content) = tokio::fs::read_to_string(path).await {
            count = content.lines().filter(|l| l.contains(pattern)).count();
            if count >= expected {
                return count;
            }
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(max_delay);
    }
    count
}

/// Wait for a file containing a single integer to reach at least `expected`.
/// Returns the actual value found (or the value at timeout).
/// Designed for watchdog-style counter files that overwrite with `echo $count > file`.
pub async fn wait_for_counter_value(path: &Path, expected: i32, timeout: Duration) -> i32 {
    let deadline = Instant::now() + timeout;
    let mut delay = Duration::from_millis(10);
    let max_delay = Duration::from_millis(500);
    let mut value = 0;

    while Instant::now() < deadline {
        if let Ok(content) = tokio::fs::read_to_string(path).await {
            if let Ok(v) = content.trim().parse::<i32>() {
                value = v;
                if value >= expected {
                    return value;
                }
            }
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(max_delay);
    }
    value
}

/// Wait for TCP port to be accepting connections
pub async fn wait_for_tcp_port(addr: &str, timeout: Duration) -> bool {
    let addr = addr.to_string();
    wait_for_condition(
        || {
            let addr = addr.clone();
            async move { tokio::net::TcpStream::connect(&addr).await.is_ok() }
        },
        timeout,
    )
    .await
}

/// Wait for Unix socket to be accepting connections
pub async fn wait_for_unix_socket(path: &Path, timeout: Duration) -> bool {
    let path = path.to_path_buf();
    wait_for_condition(
        || {
            let path = path.clone();
            async move { tokio::net::UnixStream::connect(&path).await.is_ok() }
        },
        timeout,
    )
    .await
}

// ============================================================================
// Script Generators
// ============================================================================

/// Script that echoes LISTEN_FDS environment variable to a file
pub fn script_echo_listen_fds(output_file: &Path) -> String {
    format!(
        r#"#!/bin/sh
echo "LISTEN_FDS=$LISTEN_FDS" > {}
echo "LISTEN_PID=$LISTEN_PID" >> {}
sleep 3600
"#,
        output_file.display(),
        output_file.display()
    )
}

// ============================================================================
// Test Timeouts
// ============================================================================

/// Default timeout for process startup
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);

/// Default timeout for process shutdown
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

/// Default timeout for socket connections
pub const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);
