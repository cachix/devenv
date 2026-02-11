//! Production-grade cachix daemon client with true real-time streaming
//!
//! This module implements a streaming daemon client that:
//! - Queues paths for push without blocking
//! - Processes events in a background task
//! - Automatically reconnects on daemon crashes
//! - Provides real-time metrics and observability
//! - Integrates with build/eval loops via callbacks

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Child;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

/// Connection parameters for the cachix daemon
#[derive(Clone, Debug)]
pub struct ConnectionParams {
    /// Timeout for socket connection attempts
    pub connect_timeout: Duration,
    /// Timeout for individual socket operations
    pub operation_timeout: Duration,
    /// Maximum retries for failed connections
    pub max_retries: u32,
    /// Backoff multiplier for reconnection attempts
    pub reconnect_backoff_ms: u64,
}

impl Default for ConnectionParams {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            operation_timeout: Duration::from_secs(30),
            max_retries: 3,
            reconnect_backoff_ms: 500,
        }
    }
}

/// Configuration for spawning a cachix daemon process
#[derive(Clone, Debug)]
pub struct DaemonSpawnConfig {
    /// Name of the cachix cache to push to
    pub cache_name: String,
    /// Socket path for the daemon to listen on
    pub socket_path: PathBuf,
    /// Path to the cachix binary
    pub binary: PathBuf,
    /// Run in dry-run mode (no actual uploads)
    pub dry_run: bool,
}

/// Configuration for connecting to a cachix daemon
#[derive(Clone, Debug)]
pub struct DaemonConnectConfig {
    /// Socket path to connect to
    pub socket_path: PathBuf,
    /// Connection parameters (timeouts, retries, etc.)
    pub connection: ConnectionParams,
}

impl DaemonConnectConfig {
    /// Create connect config with socket path and default connection params
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            connection: ConnectionParams::default(),
        }
    }
}

/// Real-time metrics for daemon push operations
#[derive(Debug, Clone)]
pub struct DaemonMetrics {
    /// Paths waiting in queue
    pub queued: u64,
    /// Paths currently being uploaded
    pub in_progress: u64,
    /// Paths successfully pushed
    pub completed: u64,
    /// Paths that failed to push
    pub failed: u64,
    /// Paths that encountered errors (failed + retried)
    pub failed_with_reasons: Arc<Mutex<HashMap<String, String>>>,
}

impl DaemonMetrics {
    pub fn summary(&self) -> String {
        format!(
            "Queued: {}, In Progress: {}, Completed: {}, Failed: {}",
            self.queued, self.in_progress, self.completed, self.failed
        )
    }
}

/// Trait for real-time path detection during builds
#[async_trait::async_trait]
pub trait BuildPathCallback: Send + Sync {
    /// Called when a store path is realized during evaluation
    /// Path is queued immediately, does not block
    async fn on_path_realized(&self, path: &str) -> Result<()>;

    /// Called when a build completes
    async fn on_build_complete(&self, path: &str, success: bool) -> Result<()>;
}

/// Request to push store paths to cachix
#[derive(Serialize)]
pub struct ClientPushRequest {
    pub tag: String,
    pub contents: PushRequestContents,
}

#[derive(Serialize)]
pub struct PushRequestContents {
    #[serde(rename = "storePaths")]
    pub store_paths: Vec<String>,
    #[serde(rename = "subscribeToUpdates")]
    pub subscribe_to_updates: bool,
}

impl ClientPushRequest {
    pub fn new(store_paths: Vec<String>, subscribe: bool) -> Self {
        Self {
            tag: "ClientPushRequest".to_string(),
            contents: PushRequestContents {
                store_paths,
                subscribe_to_updates: subscribe,
            },
        }
    }
}

/// Daemon event wrapper
#[derive(Debug, Deserialize)]
pub struct DaemonEvent {
    pub tag: String,
    pub contents: serde_json::Value,
}

/// Parsed push event with full structure
#[derive(Debug, Deserialize, Clone)]
pub enum PushEvent {
    #[serde(rename = "PushStarted")]
    PushStarted,

    #[serde(rename = "PushStorePathAttempt")]
    StorePathAttempt {
        path: String,
        #[serde(default)]
        size: u64,
    },

    #[serde(rename = "PushStorePathProgress")]
    StorePathProgress {
        path: String,
        bytes_uploaded: u64,
        total_bytes: u64,
    },

    #[serde(rename = "PushStorePathDone")]
    StorePathSuccess { path: String },

    #[serde(rename = "PushStorePathFailed")]
    StorePathFailed { path: String, reason: String },

    #[serde(rename = "PushFinished")]
    PushFinished {
        total_paths: u64,
        succeeded: u64,
        failed: u64,
    },

    #[serde(other)]
    Unknown,
}

/// Low-level socket client for daemon communication
struct SocketClient {
    write_half: tokio::net::unix::OwnedWriteHalf,
    buf_reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    config: DaemonConnectConfig,
}

impl SocketClient {
    /// Connect to daemon with timeout
    async fn connect(config: DaemonConnectConfig) -> Result<Self> {
        let socket = tokio::time::timeout(
            config.connection.connect_timeout,
            UnixStream::connect(&config.socket_path),
        )
        .await
        .context("Timeout connecting to cachix daemon")?
        .context("Failed to connect to cachix daemon socket")?;

        let (read_half, write_half) = socket.into_split();
        let buf_reader = BufReader::new(read_half);

        Ok(Self {
            write_half,
            buf_reader,
            config,
        })
    }

    /// Send push request to daemon
    async fn send_push_request(&mut self, paths: Vec<String>) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }

        let request = ClientPushRequest::new(paths, true);
        let json_str =
            serde_json::to_string(&request).context("Failed to serialize push request")?;

        let write_future = async {
            self.write_half
                .write_all(format!("{}\n", json_str).as_bytes())
                .await?;
            self.write_half.flush().await?;
            Ok::<(), anyhow::Error>(())
        };

        tokio::time::timeout(self.config.connection.operation_timeout, write_future)
            .await
            .context("Timeout writing push request")?
            .context("Failed to write push request")?;

        Ok(())
    }

    /// Read next event from daemon
    async fn read_event(&mut self) -> Result<Option<PushEvent>> {
        let mut line = String::new();

        let read_future = async {
            let bytes_read = self.buf_reader.read_line(&mut line).await?;

            if bytes_read == 0 {
                return Ok::<Option<PushEvent>, anyhow::Error>(None); // EOF
            }

            match serde_json::from_str::<DaemonEvent>(&line) {
                Ok(event) => match serde_json::from_value::<PushEvent>(event.contents) {
                    Ok(push_event) => Ok(Some(push_event)),
                    Err(e) => {
                        tracing::warn!("Failed to parse push event: {}", e);
                        Ok(Some(PushEvent::Unknown))
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to parse daemon event: {}", e);
                    Ok(Some(PushEvent::Unknown))
                }
            }
        };

        tokio::time::timeout(self.config.connection.operation_timeout, read_future)
            .await
            .context("Timeout reading from daemon")?
    }
}

/// Owned handle to a spawned cachix daemon process.
/// Kills the process on drop.
pub struct DaemonProcess {
    child: Option<Child>,
    socket_path: PathBuf,
}

impl DaemonProcess {
    /// Spawn a new cachix daemon process.
    ///
    /// Waits for the socket to become available before returning.
    pub async fn spawn(config: &DaemonSpawnConfig) -> Result<Self> {
        tracing::info!(cache = %config.cache_name, "Spawning cachix daemon");

        // Ensure parent directory exists for socket
        if let Some(parent) = config.socket_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create directory for daemon socket")?;
        }

        let mut cmd = std::process::Command::new(&config.binary);
        cmd.arg("daemon").arg("run");
        if config.dry_run {
            cmd.arg("--dry-run");
        }
        cmd.arg("--socket")
            .arg(&config.socket_path)
            .arg(&config.cache_name);

        let child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn cachix daemon at {:?}", config.binary))?;

        let mut daemon = Self {
            child: Some(child),
            socket_path: config.socket_path.clone(),
        };

        // Wait for socket to become available
        if let Err(e) = daemon.wait_for_socket(Duration::from_secs(10)).await {
            // Clean up on failure
            daemon.kill();
            return Err(e);
        }

        tracing::info!(socket = %config.socket_path.display(), "Daemon started");
        Ok(daemon)
    }

    /// Wait for the daemon socket to become available
    async fn wait_for_socket(&self, timeout: Duration) -> Result<()> {
        let start = Instant::now();
        let mut backoff = Duration::from_millis(100);

        while start.elapsed() < timeout {
            if self.socket_path.exists() {
                // Try to connect to verify it's ready
                if UnixStream::connect(&self.socket_path).await.is_ok() {
                    return Ok(());
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(1));
        }

        Err(anyhow!(
            "Timeout waiting for daemon socket at {}",
            self.socket_path.display()
        ))
    }

    /// Get the socket path for client connections
    pub fn socket_path(&self) -> &std::path::Path {
        &self.socket_path
    }

    /// Stop the daemon gracefully.
    ///
    /// Waits for the process to exit, or kills it after timeout.
    pub async fn stop(mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            tracing::info!("Stopping cachix daemon");

            // Try graceful shutdown first - kill the process
            // (In the future, could send ClientStop via socket)
            let _ = child.kill();

            // Wait for process to exit with timeout
            let wait_result = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => return Ok(()),
                        Ok(None) => tokio::time::sleep(Duration::from_millis(100)).await,
                        Err(e) => return Err(anyhow!("Failed to wait for daemon: {}", e)),
                    }
                }
            })
            .await;

            match wait_result {
                Ok(Ok(())) => tracing::debug!("Daemon stopped"),
                Ok(Err(e)) => tracing::warn!("Error stopping daemon: {}", e),
                Err(_) => tracing::warn!("Timeout waiting for daemon to stop"),
            }
        }
        Ok(())
    }

    /// Kill the daemon process immediately
    fn kill(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
        }
    }
}

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Combined daemon process and client.
/// Spawns the daemon and manages its full lifecycle.
pub struct OwnedDaemon {
    process: DaemonProcess,
    client: DaemonClient,
}

impl OwnedDaemon {
    /// Spawn daemon and connect client
    pub async fn spawn(config: DaemonSpawnConfig, connection: ConnectionParams) -> Result<Self> {
        let socket_path = config.socket_path.clone();
        let process = DaemonProcess::spawn(&config).await?;

        let connect_config = DaemonConnectConfig {
            socket_path,
            connection,
        };
        let client = DaemonClient::connect(connect_config).await?;

        Ok(Self { process, client })
    }

    pub async fn queue_path(&self, path: String) -> Result<()> {
        self.client.queue_path(path).await
    }

    pub async fn queue_paths(&self, paths: Vec<String>) -> Result<()> {
        self.client.queue_paths(paths).await
    }

    pub fn metrics(&self) -> DaemonMetrics {
        self.client.metrics()
    }

    pub async fn wait_for_completion(&self, timeout: Duration) -> Result<DaemonMetrics> {
        self.client.wait_for_completion(timeout).await
    }

    pub fn as_build_callback(&self) -> ClientCallback {
        self.client.as_build_callback()
    }

    /// Shutdown: wait for in-flight pushes, then stop daemon
    pub async fn shutdown(self, timeout: Duration) -> Result<()> {
        self.client.wait_for_completion(timeout).await.ok();
        self.client.shutdown();
        self.process.stop().await
    }
}

/// Client for communicating with a running cachix daemon.
pub struct DaemonClient {
    pending_paths: Arc<Mutex<VecDeque<String>>>,
    work_notify: Arc<Notify>,
    metrics: Arc<AtomicMetrics>,
    event_task: tokio::task::JoinHandle<()>,
}

/// Atomic version of metrics for background task
struct AtomicMetrics {
    queued: AtomicU64,
    in_progress: AtomicU64,
    completed: AtomicU64,
    failed: AtomicU64,
    failed_with_reasons: Arc<Mutex<HashMap<String, String>>>,
}

impl DaemonClient {
    /// Connect to an existing cachix daemon
    pub async fn connect(config: DaemonConnectConfig) -> Result<Self> {
        let client_id = Uuid::new_v4();
        tracing::debug!(client_id = %client_id, "Connecting to cachix daemon");

        let socket_client = SocketClient::connect(config.clone()).await?;

        let client = Arc::new(Mutex::new(Some(socket_client)));
        let pending_paths = Arc::new(Mutex::new(VecDeque::new()));
        let work_notify = Arc::new(Notify::new());
        let metrics = Arc::new(AtomicMetrics {
            queued: AtomicU64::new(0),
            in_progress: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            failed_with_reasons: Arc::new(Mutex::new(HashMap::new())),
        });

        let event_task = {
            let client = Arc::clone(&client);
            let pending_paths = Arc::clone(&pending_paths);
            let work_notify = Arc::clone(&work_notify);
            let metrics = Arc::clone(&metrics);
            let config = config.clone();

            tokio::spawn(async move {
                Self::event_loop(client_id, client, pending_paths, work_notify, metrics, config).await
            })
        };

        Ok(Self {
            pending_paths,
            work_notify,
            metrics,
            event_task,
        })
    }

    /// Queue a path for push (non-blocking, immediate return)
    pub async fn queue_path(&self, path: String) -> Result<()> {
        let mut queue = self.pending_paths.lock().await;
        queue.push_back(path);
        self.metrics.queued.fetch_add(1, Ordering::SeqCst);

        // Notify background task that work is available
        self.work_notify.notify_one();

        Ok(())
    }

    /// Queue multiple paths at once
    pub async fn queue_paths(&self, paths: Vec<String>) -> Result<()> {
        let count = paths.len() as u64;
        let mut queue = self.pending_paths.lock().await;

        for path in paths {
            queue.push_back(path);
        }

        self.metrics.queued.fetch_add(count, Ordering::SeqCst);
        self.work_notify.notify_one();

        Ok(())
    }

    /// Get current metrics snapshot
    pub fn metrics(&self) -> DaemonMetrics {
        DaemonMetrics {
            queued: self.metrics.queued.load(Ordering::SeqCst),
            in_progress: self.metrics.in_progress.load(Ordering::SeqCst),
            completed: self.metrics.completed.load(Ordering::SeqCst),
            failed: self.metrics.failed.load(Ordering::SeqCst),
            failed_with_reasons: Arc::clone(&self.metrics.failed_with_reasons),
        }
    }

    /// Wait for all queued paths to complete
    pub async fn wait_for_completion(&self, timeout: Duration) -> Result<DaemonMetrics> {
        let start = Instant::now();

        loop {
            if start.elapsed() > timeout {
                return Err(anyhow!("Timeout waiting for push completion"));
            }

            let metrics = self.metrics();
            if metrics.queued == 0 && metrics.in_progress == 0 {
                return Ok(metrics);
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn event_loop(
        client_id: Uuid,
        client: Arc<Mutex<Option<SocketClient>>>,
        pending_paths: Arc<Mutex<VecDeque<String>>>,
        work_notify: Arc<Notify>,
        metrics: Arc<AtomicMetrics>,
        config: DaemonConnectConfig,
    ) {
        let mut reconnect_backoff = Duration::from_millis(config.connection.reconnect_backoff_ms);

        loop {
            {
                let mut client_lock = client.lock().await;

                if client_lock.is_none() {
                    match SocketClient::connect(config.clone()).await {
                        Ok(c) => {
                            tracing::info!(client_id = %client_id, "Reconnected to daemon");
                            *client_lock = Some(c);
                            reconnect_backoff =
                                Duration::from_millis(config.connection.reconnect_backoff_ms);
                        }
                        Err(e) => {
                            tracing::warn!(client_id = %client_id, "Reconnect failed: {}", e);
                            drop(client_lock);
                            tokio::time::sleep(reconnect_backoff).await;
                            reconnect_backoff = reconnect_backoff
                                .saturating_mul(2)
                                .min(Duration::from_secs(30));
                            continue;
                        }
                    }
                }
            }

            let should_wait = Self::process_cycle(
                client_id,
                Arc::clone(&client),
                Arc::clone(&pending_paths),
                Arc::clone(&metrics),
            )
            .await;

            if should_wait {
                // Wait for work notification or timeout
                tokio::select! {
                    _ = work_notify.notified() => {
                        // Work available, continue loop
                    }
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {
                        // Periodic check even without notifications
                    }
                }
            }
        }
    }

    async fn process_cycle(
        client_id: Uuid,
        client: Arc<Mutex<Option<SocketClient>>>,
        pending_paths: Arc<Mutex<VecDeque<String>>>,
        metrics: Arc<AtomicMetrics>,
    ) -> bool {
        // Collect paths to send in this batch
        let mut paths_to_send = Vec::new();
        {
            let mut queue = pending_paths.lock().await;
            while let Some(path) = queue.pop_front() {
                paths_to_send.push(path);
                metrics.queued.fetch_sub(1, Ordering::SeqCst);
                metrics.in_progress.fetch_add(1, Ordering::SeqCst);

                // Send in small batches to avoid overwhelming daemon
                if paths_to_send.len() >= 100 {
                    break;
                }
            }
        }

        // If no work, wait for notification
        if paths_to_send.is_empty() {
            return true;
        }

        // Send paths to daemon
        let mut client_lock = client.lock().await;
        if let Some(client) = client_lock.as_mut() {
            if let Err(e) = client.send_push_request(paths_to_send.clone()).await {
                tracing::error!(client_id = %client_id, "Failed to send push request: {}", e);
                // Paths go back to queue on error
                let mut queue = pending_paths.lock().await;
                for path in paths_to_send {
                    queue.push_front(path);
                    metrics.queued.fetch_add(1, Ordering::SeqCst);
                    metrics.in_progress.fetch_sub(1, Ordering::SeqCst);
                }
                *client_lock = None; // Mark connection as dead
                return true;
            }

            // Read events for this push
            Self::read_push_events(client, &metrics, &paths_to_send).await;
        }

        false // Don't wait, immediately process more work
    }

    async fn read_push_events(
        client: &mut SocketClient,
        metrics: &Arc<AtomicMetrics>,
        sent_paths: &[String],
    ) {
        let mut paths_accounted = 0;

        loop {
            match client.read_event().await {
                Ok(Some(event)) => {
                    match event {
                        PushEvent::StorePathAttempt { path, .. } => {
                            tracing::debug!(path = %path, "Attempting to push");
                        }
                        PushEvent::StorePathProgress {
                            path,
                            bytes_uploaded,
                            total_bytes,
                        } => {
                            let percent =
                                (bytes_uploaded as f64 / total_bytes as f64 * 100.0) as u32;
                            tracing::debug!(path = %path, percent, "Upload progress");
                        }
                        PushEvent::StorePathSuccess { path } => {
                            tracing::info!(path = %path, "Push successful");
                            metrics.completed.fetch_add(1, Ordering::SeqCst);
                            metrics.in_progress.fetch_sub(1, Ordering::SeqCst);
                            paths_accounted += 1;
                        }
                        PushEvent::StorePathFailed { path, reason } => {
                            tracing::warn!(path = %path, reason = %reason, "Push failed");
                            metrics.failed.fetch_add(1, Ordering::SeqCst);
                            metrics.in_progress.fetch_sub(1, Ordering::SeqCst);

                            // Store failure reason for user visibility
                            if let Ok(mut failed_map) = metrics.failed_with_reasons.try_lock() {
                                failed_map.insert(path.clone(), reason.clone());
                            }

                            paths_accounted += 1;
                        }
                        PushEvent::PushFinished {
                            total_paths,
                            succeeded,
                            failed,
                        } => {
                            tracing::info!(
                                total = total_paths,
                                succeeded,
                                failed,
                                "Push batch completed"
                            );
                            break;
                        }
                        _ => {}
                    }

                    // If we've accounted for all paths, consider batch done
                    if paths_accounted >= sent_paths.len() {
                        break;
                    }
                }
                Ok(None) => {
                    // EOF from daemon - connection lost
                    tracing::warn!("Daemon connection lost during event reading");
                    break;
                }
                Err(e) => {
                    tracing::error!("Event read error: {}", e);
                    break;
                }
            }
        }
    }

    pub fn as_build_callback(&self) -> ClientCallback {
        ClientCallback {
            handle: self.clone_handle(),
        }
    }

    fn clone_handle(&self) -> ClientHandle {
        ClientHandle {
            pending_paths: Arc::clone(&self.pending_paths),
            work_notify: Arc::clone(&self.work_notify),
        }
    }

    pub fn shutdown(&self) {
        self.event_task.abort();
    }
}

#[derive(Clone)]
struct ClientHandle {
    pending_paths: Arc<Mutex<VecDeque<String>>>,
    work_notify: Arc<Notify>,
}

pub struct ClientCallback {
    handle: ClientHandle,
}

#[async_trait::async_trait]
impl BuildPathCallback for ClientCallback {
    async fn on_path_realized(&self, path: &str) -> Result<()> {
        let mut queue = self.handle.pending_paths.lock().await;
        queue.push_back(path.to_string());
        self.handle.work_notify.notify_one();
        Ok(())
    }

    async fn on_build_complete(&self, _path: &str, _success: bool) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_request_serialization() {
        let request =
            ClientPushRequest::new(vec!["/nix/store/abc123-package-1.0".to_string()], true);
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("ClientPushRequest"));
        assert!(json.contains("/nix/store/abc123-package-1.0"));
    }

    #[test]
    fn test_push_request_multiple_paths() {
        let request = ClientPushRequest::new(
            vec![
                "/nix/store/abc-1.0".to_string(),
                "/nix/store/def-2.0".to_string(),
            ],
            false,
        );
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("storePaths"));
        assert!(json.contains("/nix/store/abc-1.0"));
        assert!(json.contains("/nix/store/def-2.0"));
        assert!(json.contains("\"subscribeToUpdates\":false"));
    }

    #[test]
    fn test_spawn_config() {
        let config = DaemonSpawnConfig {
            cache_name: "my-cache".to_string(),
            socket_path: PathBuf::from("/tmp/test.sock"),
            binary: PathBuf::from("/nix/store/xxx-cachix/bin/cachix"),
            dry_run: true,
        };
        assert_eq!(config.cache_name, "my-cache");
        assert_eq!(config.socket_path, PathBuf::from("/tmp/test.sock"));
        assert!(config.dry_run);
    }

    #[test]
    fn test_connect_config_new() {
        let config = DaemonConnectConfig::new(PathBuf::from("/tmp/test.sock"));
        assert_eq!(config.socket_path, PathBuf::from("/tmp/test.sock"));
        assert_eq!(config.connection.connect_timeout, Duration::from_secs(5));
        assert_eq!(config.connection.max_retries, 3);
    }

    #[test]
    fn test_connect_config_custom_params() {
        let config = DaemonConnectConfig {
            socket_path: PathBuf::from("/tmp/custom.sock"),
            connection: ConnectionParams {
                connect_timeout: Duration::from_secs(10),
                operation_timeout: Duration::from_secs(60),
                max_retries: 5,
                reconnect_backoff_ms: 1000,
            },
        };
        assert_eq!(config.socket_path, PathBuf::from("/tmp/custom.sock"));
        assert_eq!(config.connection.connect_timeout, Duration::from_secs(10));
        assert_eq!(config.connection.max_retries, 5);
    }

    #[test]
    fn test_connection_params_default() {
        let params = ConnectionParams::default();
        assert_eq!(params.connect_timeout, Duration::from_secs(5));
        assert_eq!(params.operation_timeout, Duration::from_secs(30));
        assert_eq!(params.max_retries, 3);
        assert_eq!(params.reconnect_backoff_ms, 500);
    }

    #[test]
    fn test_metrics_summary_format() {
        let metrics = DaemonMetrics {
            queued: 10,
            in_progress: 5,
            completed: 100,
            failed: 2,
            failed_with_reasons: Arc::new(Mutex::new(HashMap::new())),
        };
        let summary = metrics.summary();
        assert!(summary.contains("Queued: 10"));
        assert!(summary.contains("In Progress: 5"));
        assert!(summary.contains("Completed: 100"));
        assert!(summary.contains("Failed: 2"));
    }

    #[test]
    fn test_metrics_summary_zero_values() {
        let metrics = DaemonMetrics {
            queued: 0,
            in_progress: 0,
            completed: 0,
            failed: 0,
            failed_with_reasons: Arc::new(Mutex::new(HashMap::new())),
        };
        let summary = metrics.summary();
        assert!(summary.contains("Queued: 0"));
        assert!(summary.contains("Completed: 0"));
    }

    #[test]
    fn test_push_request_empty_paths() {
        let request = ClientPushRequest::new(vec![], true);
        let json = serde_json::to_string(&request).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["contents"]["storePaths"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_push_request_roundtrip() {
        let paths = vec![
            "/nix/store/abc123-pkg".to_string(),
            "/nix/store/def456-pkg".to_string(),
        ];
        let request = ClientPushRequest::new(paths.clone(), true);
        let json = serde_json::to_string(&request).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["tag"], "ClientPushRequest");
        assert_eq!(parsed["contents"]["subscribeToUpdates"], true);
        let store_paths: Vec<String> = parsed["contents"]["storePaths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(store_paths, paths);
    }

    #[test]
    fn test_parse_daemon_event_push_started() {
        let json = r#"{"tag": "PushEvent", "contents": "PushStarted"}"#;
        let event: DaemonEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.tag, "PushEvent");
        let push: PushEvent = serde_json::from_value(event.contents).unwrap();
        assert!(matches!(push, PushEvent::PushStarted));
    }

    #[test]
    fn test_parse_daemon_event_store_path_attempt() {
        let json = r#"{"tag": "PushEvent", "contents": {"PushStorePathAttempt": {"path": "/nix/store/abc123-pkg", "size": 1024}}}"#;
        let event: DaemonEvent = serde_json::from_str(json).unwrap();
        let push: PushEvent = serde_json::from_value(event.contents).unwrap();
        match push {
            PushEvent::StorePathAttempt { path, size } => {
                assert_eq!(path, "/nix/store/abc123-pkg");
                assert_eq!(size, 1024);
            }
            other => panic!("Expected StorePathAttempt, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_daemon_event_store_path_progress() {
        let json = r#"{"tag": "PushEvent", "contents": {"PushStorePathProgress": {"path": "/nix/store/abc123-pkg", "bytes_uploaded": 512, "total_bytes": 1024}}}"#;
        let event: DaemonEvent = serde_json::from_str(json).unwrap();
        let push: PushEvent = serde_json::from_value(event.contents).unwrap();
        match push {
            PushEvent::StorePathProgress { path, bytes_uploaded, total_bytes } => {
                assert_eq!(path, "/nix/store/abc123-pkg");
                assert_eq!(bytes_uploaded, 512);
                assert_eq!(total_bytes, 1024);
            }
            other => panic!("Expected StorePathProgress, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_daemon_event_store_path_done() {
        let json = r#"{"tag": "PushEvent", "contents": {"PushStorePathDone": {"path": "/nix/store/abc123-pkg"}}}"#;
        let event: DaemonEvent = serde_json::from_str(json).unwrap();
        let push: PushEvent = serde_json::from_value(event.contents).unwrap();
        match push {
            PushEvent::StorePathSuccess { path } => {
                assert_eq!(path, "/nix/store/abc123-pkg");
            }
            other => panic!("Expected StorePathSuccess, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_daemon_event_store_path_failed() {
        let json = r#"{"tag": "PushEvent", "contents": {"PushStorePathFailed": {"path": "/nix/store/abc123-pkg", "reason": "upload timeout"}}}"#;
        let event: DaemonEvent = serde_json::from_str(json).unwrap();
        let push: PushEvent = serde_json::from_value(event.contents).unwrap();
        match push {
            PushEvent::StorePathFailed { path, reason } => {
                assert_eq!(path, "/nix/store/abc123-pkg");
                assert_eq!(reason, "upload timeout");
            }
            other => panic!("Expected StorePathFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_daemon_event_push_finished() {
        let json = r#"{"tag": "PushEvent", "contents": {"PushFinished": {"total_paths": 10, "succeeded": 8, "failed": 2}}}"#;
        let event: DaemonEvent = serde_json::from_str(json).unwrap();
        let push: PushEvent = serde_json::from_value(event.contents).unwrap();
        match push {
            PushEvent::PushFinished { total_paths, succeeded, failed } => {
                assert_eq!(total_paths, 10);
                assert_eq!(succeeded, 8);
                assert_eq!(failed, 2);
            }
            other => panic!("Expected PushFinished, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_daemon_event_unknown_string_variant() {
        let json = r#"{"tag": "PushEvent", "contents": "SomeNewEvent"}"#;
        let event: DaemonEvent = serde_json::from_str(json).unwrap();
        let push: PushEvent = serde_json::from_value(event.contents).unwrap();
        assert!(matches!(push, PushEvent::Unknown));
    }

    #[test]
    fn test_parse_daemon_event_unknown_map_variant_fails() {
        let json = r#"{"tag": "PushEvent", "contents": {"SomeNewEvent": {"foo": "bar"}}}"#;
        let event: DaemonEvent = serde_json::from_str(json).unwrap();
        assert!(serde_json::from_value::<PushEvent>(event.contents).is_err());
    }

    #[test]
    fn test_parse_daemon_event_attempt_missing_size_defaults() {
        let json = r#"{"tag": "PushEvent", "contents": {"PushStorePathAttempt": {"path": "/nix/store/abc123-pkg"}}}"#;
        let event: DaemonEvent = serde_json::from_str(json).unwrap();
        let push: PushEvent = serde_json::from_value(event.contents).unwrap();
        match push {
            PushEvent::StorePathAttempt { path, size } => {
                assert_eq!(path, "/nix/store/abc123-pkg");
                assert_eq!(size, 0);
            }
            other => panic!("Expected StorePathAttempt, got {:?}", other),
        }
    }

    #[test]
    fn test_spawn_config_dry_run_false() {
        let config = DaemonSpawnConfig {
            cache_name: "my-cache".to_string(),
            socket_path: PathBuf::from("/tmp/test.sock"),
            binary: PathBuf::from("cachix"),
            dry_run: false,
        };
        assert!(!config.dry_run);
    }
}
