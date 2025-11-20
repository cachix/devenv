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
use std::env;
use std::path::PathBuf;
use std::process::Child;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

/// Configuration for cachix daemon connection and behavior
#[derive(Clone, Debug)]
pub struct DaemonConfig {
    /// Timeout for socket connection attempts
    pub connect_timeout: Duration,
    /// Timeout for individual socket operations
    pub operation_timeout: Duration,
    /// Maximum retries for failed connections
    pub max_retries: u32,
    /// Backoff multiplier for reconnection attempts
    pub reconnect_backoff_ms: u64,
    /// Optional socket path for testing (overrides env vars)
    pub socket_path: Option<PathBuf>,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            operation_timeout: Duration::from_secs(30),
            max_retries: 3,
            reconnect_backoff_ms: 500,
            socket_path: None,
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

/// Low-level socket client
struct CachixDaemonClient {
    write_half: tokio::net::unix::OwnedWriteHalf,
    buf_reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    config: DaemonConfig,
}

impl CachixDaemonClient {
    /// Get the cachix daemon socket path
    fn get_socket_path(config: &DaemonConfig) -> Result<PathBuf> {
        // Check if socket path is explicitly provided in config (for testing)
        if let Some(socket_path) = &config.socket_path {
            return Ok(socket_path.clone());
        }

        if let Ok(socket_path) = env::var("CACHIX_DAEMON_SOCKET") {
            return Ok(PathBuf::from(socket_path));
        }

        if let Ok(runtime_dir) = env::var("XDG_RUNTIME_DIR") {
            let socket_path = PathBuf::from(runtime_dir)
                .join("cachix")
                .join("cachix-daemon.sock");
            if socket_path.exists() {
                return Ok(socket_path);
            }
        }

        if let Ok(cache_home) = env::var("XDG_CACHE_HOME") {
            let socket_path = PathBuf::from(cache_home)
                .join("cachix")
                .join("cachix-daemon.sock");
            if socket_path.exists() {
                return Ok(socket_path);
            }
        }

        Err(anyhow!(
            "Cachix daemon socket not found. \
             Set CACHIX_DAEMON_SOCKET or ensure XDG_RUNTIME_DIR is configured"
        ))
    }

    /// Connect to daemon with timeout
    async fn connect(config: DaemonConfig) -> Result<Self> {
        let socket_path = Self::get_socket_path(&config)?;

        let socket =
            tokio::time::timeout(config.connect_timeout, UnixStream::connect(&socket_path))
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

        tokio::time::timeout(self.config.operation_timeout, write_future)
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

        tokio::time::timeout(self.config.operation_timeout, read_future)
            .await
            .context("Timeout reading from daemon")?
    }
}

/// Production streaming daemon with background event processing
pub struct StreamingCachixDaemon {
    /// Unique ID for this daemon instance
    daemon_id: Uuid,
    /// Daemon process (if we spawned it)
    daemon_process: Arc<Mutex<Option<Child>>>,
    /// Queue of paths awaiting push
    pending_paths: Arc<Mutex<VecDeque<String>>>,
    /// Notifier to wake background task
    work_notify: Arc<Notify>,
    /// Metrics updated by background task
    metrics: Arc<AtomicMetrics>,
    /// Background task handle
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

impl StreamingCachixDaemon {
    /// Start or connect to cachix daemon
    pub async fn start(config: DaemonConfig) -> Result<Self> {
        let daemon_id = Uuid::new_v4();
        tracing::info!(daemon_id = %daemon_id, "Starting cachix daemon");

        // Try to connect to existing daemon first
        let client = match CachixDaemonClient::connect(config.clone()).await {
            Ok(c) => {
                tracing::debug!(daemon_id = %daemon_id, "Connected to existing daemon");
                Some(c)
            }
            Err(e) => {
                tracing::debug!(daemon_id = %daemon_id, "No existing daemon found: {}", e);
                None
            }
        };

        // If no existing daemon, try to start one
        let (client, daemon_process) = if client.is_none() {
            let process = std::process::Command::new("cachix")
                .arg("daemon")
                .arg("run")
                .spawn()
                .context("Failed to spawn cachix daemon. Is cachix CLI installed?")?;

            // Poll for socket with exponential backoff
            let mut retries = 0;
            let mut backoff = Duration::from_millis(config.reconnect_backoff_ms);

            let client = loop {
                tokio::time::sleep(backoff).await;

                match CachixDaemonClient::connect(config.clone()).await {
                    Ok(c) => {
                        tracing::info!(daemon_id = %daemon_id, retries, "Daemon started successfully");
                        break c;
                    }
                    Err(_) if retries < config.max_retries => {
                        retries += 1;
                        backoff = backoff.saturating_mul(2).min(Duration::from_secs(5));
                        continue;
                    }
                    Err(e) => return Err(e).context("Failed to connect to daemon after startup"),
                }
            };

            (Some(client), Some(process))
        } else {
            (client, None)
        };

        let client = Arc::new(Mutex::new(client));
        let pending_paths = Arc::new(Mutex::new(VecDeque::new()));
        let work_notify = Arc::new(Notify::new());
        let metrics = Arc::new(AtomicMetrics {
            queued: AtomicU64::new(0),
            in_progress: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            failed_with_reasons: Arc::new(Mutex::new(HashMap::new())),
        });

        // Spawn background event processing task
        let event_task = {
            let daemon_id = daemon_id.clone();
            let client = Arc::clone(&client);
            let pending_paths = Arc::clone(&pending_paths);
            let work_notify = Arc::clone(&work_notify);
            let metrics = Arc::clone(&metrics);
            let config = config.clone();

            tokio::spawn(async move {
                Self::event_loop(
                    daemon_id,
                    client,
                    pending_paths,
                    work_notify,
                    metrics,
                    config,
                )
                .await
            })
        };

        Ok(Self {
            daemon_id,
            daemon_process: Arc::new(Mutex::new(daemon_process)),
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

    /// Background event processing loop
    async fn event_loop(
        daemon_id: Uuid,
        client: Arc<Mutex<Option<CachixDaemonClient>>>,
        pending_paths: Arc<Mutex<VecDeque<String>>>,
        work_notify: Arc<Notify>,
        metrics: Arc<AtomicMetrics>,
        config: DaemonConfig,
    ) {
        let mut reconnect_backoff = Duration::from_millis(config.reconnect_backoff_ms);

        loop {
            // Try to ensure we have a connected client
            {
                let mut client_lock = client.lock().await;

                if client_lock.is_none() {
                    match CachixDaemonClient::connect(config.clone()).await {
                        Ok(c) => {
                            tracing::info!(daemon_id = %daemon_id, "Reconnected to daemon");
                            *client_lock = Some(c);
                            reconnect_backoff = Duration::from_millis(config.reconnect_backoff_ms);
                        }
                        Err(e) => {
                            tracing::warn!(daemon_id = %daemon_id, "Reconnect failed: {}", e);
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

            // Process pending paths and read events
            let should_wait = Self::process_cycle(
                daemon_id.clone(),
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

    /// Single iteration of event processing
    async fn process_cycle(
        daemon_id: Uuid,
        client: Arc<Mutex<Option<CachixDaemonClient>>>,
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
                tracing::error!(daemon_id = %daemon_id, "Failed to send push request: {}", e);
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

    /// Read events until push completes
    async fn read_push_events(
        client: &mut CachixDaemonClient,
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

    /// Get callback for build integration
    pub fn as_build_callback(&self) -> StreamingCallback {
        StreamingCallback {
            daemon: self.clone_handle(),
        }
    }

    fn clone_handle(&self) -> StreamingDaemonHandle {
        StreamingDaemonHandle {
            pending_paths: Arc::clone(&self.pending_paths),
            work_notify: Arc::clone(&self.work_notify),
        }
    }

    /// Shutdown daemon gracefully
    pub async fn shutdown(&mut self) -> Result<()> {
        tracing::info!(daemon_id = %self.daemon_id, "Shutting down daemon");

        // Wait for any in-progress work
        self.wait_for_completion(Duration::from_secs(60)).await.ok();

        // Kill daemon process if we spawned it
        let mut process_lock = self.daemon_process.lock().await;
        if let Some(mut process) = process_lock.take() {
            let _ = process.kill();
        }

        // Cancel background task
        self.event_task.abort();

        Ok(())
    }
}

/// Cloneable handle for callback integration
#[derive(Clone)]
struct StreamingDaemonHandle {
    pending_paths: Arc<Mutex<VecDeque<String>>>,
    work_notify: Arc<Notify>,
}

#[async_trait::async_trait]
impl BuildPathCallback for StreamingCallback {
    async fn on_path_realized(&self, path: &str) -> Result<()> {
        let mut queue = self.daemon.pending_paths.lock().await;
        queue.push_back(path.to_string());
        self.daemon.work_notify.notify_one();
        Ok(())
    }

    async fn on_build_complete(&self, _path: &str, _success: bool) -> Result<()> {
        Ok(())
    }
}

/// Callback implementation for real-time path detection
pub struct StreamingCallback {
    daemon: StreamingDaemonHandle,
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

    #[tokio::test]
    async fn test_queue_and_metrics() {
        let config = DaemonConfig::default();
        let metrics = Arc::new(AtomicMetrics {
            queued: AtomicU64::new(0),
            in_progress: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            failed_with_reasons: Arc::new(Mutex::new(HashMap::new())),
        });

        metrics.queued.fetch_add(5, Ordering::SeqCst);
        assert_eq!(metrics.queued.load(Ordering::SeqCst), 5);

        metrics.queued.fetch_sub(2, Ordering::SeqCst);
        metrics.in_progress.fetch_add(2, Ordering::SeqCst);
        assert_eq!(metrics.queued.load(Ordering::SeqCst), 3);
        assert_eq!(metrics.in_progress.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_daemon_config_default() {
        let config = DaemonConfig::default();
        assert_eq!(config.connect_timeout, Duration::from_secs(5));
        assert_eq!(config.operation_timeout, Duration::from_secs(30));
        assert_eq!(config.max_retries, 3);
    }
}
