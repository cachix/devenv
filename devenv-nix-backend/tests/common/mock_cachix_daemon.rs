//! Mock cachix daemon for testing
//!
//! Simulates the cachix daemon protocol over unix socket

use serde_json::json;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

/// Mock daemon that responds to push requests with simulated events
pub struct MockCachixDaemon {
    listener: UnixListener,
    socket_path: std::path::PathBuf,
    /// Paths that have been pushed (shared state for verification)
    pub pushed_paths: Arc<Mutex<Vec<String>>>,
    /// Keep tempdir alive for the socket
    _temp_dir: tempfile::TempDir,
}

impl MockCachixDaemon {
    /// Start a mock daemon on a temporary socket
    pub async fn start() -> std::io::Result<Self> {
        let temp_dir = tempfile::tempdir()?;
        let socket_path = temp_dir.path().join("cachix-daemon.sock");

        let listener = UnixListener::bind(&socket_path)?;

        Ok(Self {
            listener,
            socket_path,
            pushed_paths: Arc::new(Mutex::new(Vec::new())),
            _temp_dir: temp_dir,
        })
    }

    /// Accept a single client connection and handle push requests
    /// Returns when client disconnects
    pub async fn accept_and_handle(&self) -> std::io::Result<()> {
        let (stream, _) = self.listener.accept().await?;
        let pushed_paths = Arc::clone(&self.pushed_paths);
        Self::handle_client(stream, pushed_paths).await
    }

    /// Spawn a background task that handles multiple client connections
    /// Returns a JoinHandle and must keep the MockCachixDaemon alive
    pub fn spawn_handler(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let mock = Arc::clone(self);

        tokio::spawn(async move {
            loop {
                match mock.listener.accept().await {
                    Ok((stream, _)) => {
                        let paths = Arc::clone(&mock.pushed_paths);
                        tokio::spawn(async move {
                            let _ = Self::handle_client(stream, paths).await;
                        });
                    }
                    Err(e) => {
                        eprintln!("Mock daemon accept error: {}", e);
                        break;
                    }
                }
            }
        })
    }

    /// Handle a client connection
    async fn handle_client(
        stream: UnixStream,
        pushed_paths: Arc<Mutex<Vec<String>>>,
    ) -> std::io::Result<()> {
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await?;

            if bytes_read == 0 {
                break; // EOF
            }

            // Parse the request
            if let Ok(request) = serde_json::from_str::<serde_json::Value>(&line) {
                if request["tag"] == "ClientPushRequest" {
                    let store_paths = request["contents"]["storePaths"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();

                    eprintln!("Mock daemon received {} paths", store_paths.len());

                    // Store paths for verification
                    {
                        let mut paths = pushed_paths.lock().await;
                        paths.extend(store_paths.clone());
                    }

                    // Simulate push events for each path
                    Self::send_push_started(&mut write_half).await?;

                    for path in &store_paths {
                        Self::send_attempt(&mut write_half, path).await?;
                        Self::send_progress(&mut write_half, path, 50).await?;
                        Self::send_progress(&mut write_half, path, 100).await?;
                        Self::send_success(&mut write_half, path).await?;
                    }

                    let total = store_paths.len() as u64;
                    Self::send_finished(&mut write_half, total, total, 0).await?;
                }
            }
        }

        Ok(())
    }

    async fn send_push_started(
        write: &mut tokio::net::unix::OwnedWriteHalf,
    ) -> std::io::Result<()> {
        let event = json!({
            "tag": "PushEvent",
            "contents": "PushStarted"
        });
        Self::send_event(write, event).await
    }

    async fn send_attempt(
        write: &mut tokio::net::unix::OwnedWriteHalf,
        path: &str,
    ) -> std::io::Result<()> {
        let event = json!({
            "tag": "PushEvent",
            "contents": {
                "PushStorePathAttempt": {
                    "path": path,
                    "size": 1024
                }
            }
        });
        Self::send_event(write, event).await
    }

    async fn send_progress(
        write: &mut tokio::net::unix::OwnedWriteHalf,
        path: &str,
        percent: u64,
    ) -> std::io::Result<()> {
        let event = json!({
            "tag": "PushEvent",
            "contents": {
                "PushStorePathProgress": {
                    "path": path,
                    "bytes_uploaded": percent * 1024 / 100,
                    "total_bytes": 1024
                }
            }
        });
        Self::send_event(write, event).await
    }

    async fn send_success(
        write: &mut tokio::net::unix::OwnedWriteHalf,
        path: &str,
    ) -> std::io::Result<()> {
        let event = json!({
            "tag": "PushEvent",
            "contents": {
                "PushStorePathDone": {
                    "path": path
                }
            }
        });
        Self::send_event(write, event).await
    }

    async fn send_finished(
        write: &mut tokio::net::unix::OwnedWriteHalf,
        total: u64,
        succeeded: u64,
        failed: u64,
    ) -> std::io::Result<()> {
        let event = json!({
            "tag": "PushEvent",
            "contents": {
                "PushFinished": {
                    "total_paths": total,
                    "succeeded": succeeded,
                    "failed": failed
                }
            }
        });
        Self::send_event(write, event).await
    }

    async fn send_event(
        write: &mut tokio::net::unix::OwnedWriteHalf,
        event: serde_json::Value,
    ) -> std::io::Result<()> {
        let json_str = serde_json::to_string(&event)?;
        write
            .write_all(format!("{}\n", json_str).as_bytes())
            .await?;
        write.flush().await?;
        Ok(())
    }

    pub fn socket_path(&self) -> &std::path::Path {
        &self.socket_path
    }

    /// Get a copy of the pushed paths
    pub async fn get_pushed_paths(&self) -> Vec<String> {
        self.pushed_paths.lock().await.clone()
    }
}

impl Drop for MockCachixDaemon {
    fn drop(&mut self) {
        // Clean up socket file (tempdir is dropped automatically via _temp_dir)
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
