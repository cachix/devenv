//! Production-grade cachix daemon client with true real-time streaming
//!
//! This module implements a streaming daemon client that:
//! - Queues paths for push without blocking
//! - Processes events in a background task
//! - Automatically reconnects on daemon crashes
//! - Provides real-time metrics and observability
//! - Integrates with build/eval loops via callbacks
//! - Reports progress to the TUI via Activity

use anyhow::{Context, Result, anyhow};
use devenv_activity::{Activity, activity};
use devenv_core::nix_log_bridge::extract_package_name;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::process::{Child, Stdio};
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

/// Real-time metrics for daemon push operations.
///
/// `in_progress` counts push requests the daemon has accepted but not
/// yet `PushFinished` — *not* per-path. The daemon expands each request
/// into its closure server-side and only emits per-path events for the
/// missing subset, so any per-path "in progress" counter would be
/// structurally wrong. `PushFinished` is the only event guaranteed once
/// per request.
#[derive(Debug, Clone)]
pub struct DaemonMetrics {
    /// Paths waiting in our local queue (not yet sent to the daemon).
    pub queued: u64,
    /// Push requests sent to the daemon but not yet acknowledged via `PushFinished`.
    pub in_progress: u64,
    /// Running count of distinct paths the daemon has begun attempting
    /// to push, deduped per request so retries don't double-count.
    pub total_expected: u64,
    /// Paths the daemon reported as successfully pushed.
    pub completed: u64,
    /// Paths the daemon reported as already cached (no upload performed).
    pub skipped: u64,
    /// Paths the daemon reported as failed.
    pub failed: u64,
    /// Failure reasons keyed by path.
    pub failed_with_reasons: Arc<Mutex<HashMap<String, String>>>,
}

impl DaemonMetrics {
    pub fn summary(&self) -> String {
        format!(
            "Queued: {}, In Progress: {}, Expected: {}, Completed: {}, Skipped: {}, Failed: {}",
            self.queued,
            self.in_progress,
            self.total_expected,
            self.completed,
            self.skipped,
            self.failed
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

use crate::cachix_protocol::{ClientPushRequest, DaemonMessage, PushEvent, PushEventEnvelope};

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

    /// Read next push event from daemon, returning None for EOF or non-push messages.
    async fn read_event(&mut self) -> Result<Option<PushEvent>> {
        let mut line = String::new();

        let read_future = async {
            let bytes_read = self.buf_reader.read_line(&mut line).await?;

            if bytes_read == 0 {
                return Ok::<Option<PushEvent>, anyhow::Error>(None);
            }

            let msg: DaemonMessage = match serde_json::from_str(&line) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("Failed to parse daemon message: {}", e);
                    return Ok(Some(PushEvent::Unknown));
                }
            };

            if msg.tag != "DaemonPushEvent" {
                return Ok(None);
            }

            let envelope: PushEventEnvelope = match serde_json::from_value(msg.contents) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Failed to parse push event envelope: {}", e);
                    return Ok(Some(PushEvent::Unknown));
                }
            };

            Ok(Some(PushEvent::parse(&envelope.message)))
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

        let mut child = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to spawn cachix daemon at {:?}", config.binary))?;

        // Drain stderr in a background thread so daemon logs don't leak into the TUI.
        if let Some(stderr) = child.stderr.take() {
            std::thread::Builder::new()
                .name("cachix-stderr".into())
                .spawn(move || {
                    use std::io::BufRead;
                    let reader = std::io::BufReader::new(stderr);
                    for line in reader.lines() {
                        match line {
                            Ok(line) => {
                                tracing::debug!(target: "cachix_daemon", "{}", line);
                            }
                            Err(_) => break,
                        }
                    }
                })
                .expect("failed to spawn cachix-stderr thread");
        }

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
            if UnixStream::connect(&self.socket_path).await.is_ok() {
                return Ok(());
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
        let cache_name = config.cache_name.clone();
        let socket_path = config.socket_path.clone();
        let process = DaemonProcess::spawn(&config).await?;

        let connect_config = DaemonConnectConfig {
            socket_path,
            connection,
        };
        let client = DaemonClient::connect(connect_config, cache_name).await?;

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

    /// Shutdown: wait for pending pushes to drain, then stop daemon
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
    total_expected: AtomicU64,
    completed: AtomicU64,
    /// Paths the daemon reported as already in the cache (no upload needed).
    skipped: AtomicU64,
    failed: AtomicU64,
    failed_with_reasons: Arc<Mutex<HashMap<String, String>>>,
}

impl AtomicMetrics {
    fn report_progress(&self, activity: &Activity, current_path: Option<&str>) {
        let done = self.completed.load(Ordering::SeqCst)
            + self.skipped.load(Ordering::SeqCst)
            + self.failed.load(Ordering::SeqCst);
        let expected = self.total_expected.load(Ordering::SeqCst);
        activity.progress(done, expected, current_path);
    }
}

impl DaemonClient {
    /// Connect to an existing cachix daemon
    pub async fn connect(config: DaemonConnectConfig, cache_name: String) -> Result<Self> {
        let client_id = Uuid::new_v4();
        tracing::debug!(client_id = %client_id, "Connecting to cachix daemon");

        let socket_client = SocketClient::connect(config.clone()).await?;

        let client = Arc::new(Mutex::new(Some(socket_client)));
        let pending_paths = Arc::new(Mutex::new(VecDeque::new()));
        let work_notify = Arc::new(Notify::new());
        let metrics = Arc::new(AtomicMetrics {
            queued: AtomicU64::new(0),
            in_progress: AtomicU64::new(0),
            total_expected: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            skipped: AtomicU64::new(0),
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
                Self::event_loop(
                    client_id,
                    client,
                    pending_paths,
                    work_notify,
                    metrics,
                    config,
                    cache_name,
                )
                .await
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
            total_expected: self.metrics.total_expected.load(Ordering::SeqCst),
            completed: self.metrics.completed.load(Ordering::SeqCst),
            skipped: self.metrics.skipped.load(Ordering::SeqCst),
            failed: self.metrics.failed.load(Ordering::SeqCst),
            failed_with_reasons: Arc::clone(&self.metrics.failed_with_reasons),
        }
    }

    /// Wait until our queue is empty and the daemon has acknowledged
    /// `PushFinished` for every push request we sent.
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
        cache_name: String,
    ) {
        let mut reconnect_backoff = Duration::from_millis(config.connection.reconnect_backoff_ms);
        // TODO(cachix-ux): pushes run async to shell entry, so this spinner
        // can render above the shell prompt for ongoing uploads — confusing
        // UX. Options: (1) hide push activity from the TUI entirely and surface
        // results via the post-exit summary only, (2) switch to a Process
        // activity (no spinner, stable status line), or (3) render push
        // state in the existing devenv-shell status line. Leaning (1).
        let mut current_activity: Option<Activity> = None;

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

            if current_activity.is_none() && metrics.queued.load(Ordering::SeqCst) > 0 {
                current_activity = Some(activity!(
                    INFO,
                    operation,
                    format!("Pushing to {cache_name}")
                ));
            }

            let should_wait = Self::process_cycle(
                client_id,
                Arc::clone(&client),
                Arc::clone(&pending_paths),
                Arc::clone(&metrics),
                current_activity.as_ref(),
            )
            .await;

            // `should_wait` means process_cycle found nothing to send and no
            // request is currently being read. Combined with nothing in
            // progress at the daemon, that's true idle — drop the activity.
            if should_wait
                && current_activity.is_some()
                && metrics.in_progress.load(Ordering::SeqCst) == 0
            {
                current_activity = None;
            }

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
        activity: Option<&Activity>,
    ) -> bool {
        // Collect paths to send in this push request — capped to avoid
        // overwhelming the daemon.
        const MAX_PATHS_PER_REQUEST: usize = 100;
        let paths_to_send: Vec<String> = {
            let mut queue = pending_paths.lock().await;
            let n = queue.len().min(MAX_PATHS_PER_REQUEST);
            metrics.queued.fetch_sub(n as u64, Ordering::SeqCst);
            queue.drain(..n).collect()
        };

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
                }
                *client_lock = None; // Mark connection as dead
                return true;
            }

            metrics.in_progress.fetch_add(1, Ordering::SeqCst);
            Self::read_push_events(client, &metrics, activity).await;
        }

        false // Don't wait, immediately process more work
    }

    /// Read push events for one in-progress request until `PushFinished`,
    /// EOF, or a read error. Decrements `in_progress` exactly once before
    /// returning, regardless of exit reason — so a dropped or abnormally
    /// terminated request can't leak the counter.
    ///
    /// Bumps `total_expected` on the *first* `StorePathAttempt` for each
    /// path; retries re-fire `StorePathAttempt` with `retry_count > 0` and
    /// must not double-count. This is the only client-visible signal for
    /// closure size — the daemon resolves closures server-side and doesn't
    /// emit a count.
    async fn read_push_events(
        client: &mut SocketClient,
        metrics: &Arc<AtomicMetrics>,
        activity: Option<&Activity>,
    ) {
        let mut attempted: HashSet<String> = HashSet::new();

        loop {
            match client.read_event().await {
                Ok(Some(event)) => match event {
                    PushEvent::StorePathAttempt { ref path, .. } => {
                        tracing::debug!(path = %path, "Attempting to push");
                        if attempted.insert(path.clone()) {
                            metrics.total_expected.fetch_add(1, Ordering::SeqCst);
                        }
                        if let Some(activity) = activity {
                            let name = extract_package_name(path);
                            metrics.report_progress(activity, Some(&name));
                        }
                    }
                    PushEvent::StorePathProgress {
                        ref path,
                        current_bytes,
                        ..
                    } => {
                        tracing::debug!(path = %path, current_bytes, "Upload progress");
                    }
                    PushEvent::StorePathDone { ref path } => {
                        tracing::info!(path = %path, "Push successful");
                        metrics.completed.fetch_add(1, Ordering::SeqCst);
                        if let Some(activity) = activity {
                            metrics.report_progress(activity, None);
                        }
                    }
                    PushEvent::StorePathSkipped { ref path } => {
                        tracing::debug!(path = %path, "Already cached, skipped");
                        metrics.skipped.fetch_add(1, Ordering::SeqCst);
                        if let Some(activity) = activity {
                            metrics.report_progress(activity, None);
                        }
                    }
                    PushEvent::StorePathFailed {
                        ref path,
                        ref reason,
                    } => {
                        tracing::warn!(path = %path, reason = %reason, "Push failed");
                        metrics.failed.fetch_add(1, Ordering::SeqCst);
                        metrics
                            .failed_with_reasons
                            .lock()
                            .await
                            .insert(path.clone(), reason.clone());

                        if let Some(activity) = activity {
                            activity.error(format!("{}: {}", path, reason));
                            metrics.report_progress(activity, None);
                        }
                    }
                    PushEvent::PushFinished => {
                        tracing::info!("Push request completed");
                        break;
                    }
                    _ => {}
                },
                Ok(None) => {
                    tracing::warn!("Daemon connection lost during event reading");
                    break;
                }
                Err(e) => {
                    tracing::error!("Event read error: {}", e);
                    break;
                }
            }
        }

        metrics.in_progress.fetch_sub(1, Ordering::SeqCst);
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

    // Wire-protocol parsing/serialization tests live in
    // `cachix_protocol::tests`. This module only covers daemon
    // configuration and metrics shaping.

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
            in_progress: 2,
            total_expected: 150,
            completed: 100,
            skipped: 8,
            failed: 2,
            failed_with_reasons: Arc::new(Mutex::new(HashMap::new())),
        };
        let summary = metrics.summary();
        assert!(summary.contains("Queued: 10"));
        assert!(summary.contains("In Progress: 2"));
        assert!(summary.contains("Expected: 150"));
        assert!(summary.contains("Completed: 100"));
        assert!(summary.contains("Skipped: 8"));
        assert!(summary.contains("Failed: 2"));
    }

    #[test]
    fn test_metrics_summary_zero_values() {
        let metrics = DaemonMetrics {
            queued: 0,
            in_progress: 0,
            total_expected: 0,
            completed: 0,
            skipped: 0,
            failed: 0,
            failed_with_reasons: Arc::new(Mutex::new(HashMap::new())),
        };
        let summary = metrics.summary();
        assert!(summary.contains("Queued: 0"));
        assert!(summary.contains("Completed: 0"));
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
