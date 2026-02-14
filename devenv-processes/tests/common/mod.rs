//! Shared test utilities for devenv-processes integration tests.

// Each test file compiles separately, so not all helpers are used in each binary
#![allow(dead_code)]

use devenv_processes::{
    ListenKind, ListenSpec, NativeProcessManager, ProcessConfig, RestartConfig, RestartPolicy,
    WatchConfig,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::fs;

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

    /// Create a NativeProcessManager
    pub fn create_manager(&self) -> NativeProcessManager {
        NativeProcessManager::new(self.state_dir.clone()).expect("Failed to create manager")
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
        restart: RestartConfig {
            on: RestartPolicy::Never,
            ..Default::default()
        },
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
        restart: RestartConfig {
            on: RestartPolicy::Never,
            ..Default::default()
        },
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
        restart: RestartConfig {
            on: RestartPolicy::Never,
            ..Default::default()
        },
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
        restart: RestartConfig {
            on: RestartPolicy::Never,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Create watch config with extension filter
pub fn watch_process_config_with_extensions(
    name: &str,
    script_path: &Path,
    watch_paths: Vec<PathBuf>,
    extensions: Vec<String>,
) -> ProcessConfig {
    ProcessConfig {
        name: name.to_string(),
        exec: script_path.to_string_lossy().to_string(),
        args: vec![],
        watch: WatchConfig {
            paths: watch_paths,
            extensions,
            ignore: vec![],
        },
        restart: RestartConfig {
            on: RestartPolicy::Never,
            max: None,
        },
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
printf "LISTEN_FDS=$LISTEN_FDS\nLISTEN_PID=$LISTEN_PID\n" > {}
sleep 3600
"#,
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

// ============================================================================
// File Watcher Helpers
// ============================================================================

/// Probe the file watcher until it's live by repeatedly writing to a watched
/// path until the process restarts, proving the OS watcher is active.
/// Returns the new start count.
///
/// FSEvents on macOS sets up asynchronously — there is no API to know when the
/// OS watcher is actually ready. This helper replaces fixed sleeps by writing
/// to `probe_path` every 500 ms until `counter_file` shows more than
/// `current_count` lines matching `pattern`.
pub async fn wait_for_watcher_ready(
    probe_path: &Path,
    counter_file: &Path,
    pattern: &str,
    current_count: usize,
    timeout: Duration,
) -> usize {
    let deadline = Instant::now() + timeout;
    let mut probe_num = 0u64;

    while Instant::now() < deadline {
        probe_num += 1;
        tokio::fs::write(probe_path, format!("probe {probe_num}"))
            .await
            .expect("Failed to write probe file");

        let remaining = deadline.saturating_duration_since(Instant::now());
        let poll_time = Duration::from_millis(500).min(remaining);
        let count = wait_for_line_count(counter_file, pattern, current_count + 1, poll_time).await;
        if count > current_count {
            return count;
        }
    }
    current_count
}
