use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// HTTP GET readiness probe that polls a URL until it returns a 2xx status.
///
/// Spawns a background task that periodically sends HTTP GET requests. Call
/// `recv()` to wait for the probe to succeed. Drop the probe to cancel the
/// background task.
pub struct HttpGetProbe {
    rx: mpsc::Receiver<()>,
    task: JoinHandle<()>,
}

impl HttpGetProbe {
    /// Spawn a new HTTP GET probe that polls `url` every `period` seconds,
    /// with an `initial_delay` before the first attempt and a per-request
    /// `timeout`.
    pub fn spawn(
        url: String,
        name: String,
        initial_delay: Duration,
        period: Duration,
        timeout: Duration,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<()>(1);
        let task = tokio::spawn(async move {
            debug!("Starting HTTP probe for {} at {}", name, url);

            if !initial_delay.is_zero() {
                tokio::time::sleep(initial_delay).await;
            }

            let client = reqwest::Client::builder()
                .timeout(timeout)
                .danger_accept_invalid_certs(true)
                .build()
                .expect("failed to build HTTP client");

            loop {
                match client.get(&url).send().await {
                    Ok(response) if response.status().is_success() => {
                        info!(
                            "HTTP probe succeeded for {} at {} ({})",
                            name,
                            url,
                            response.status()
                        );
                        let _ = tx.send(()).await;
                        break;
                    }
                    Ok(response) => {
                        debug!(
                            "HTTP probe for {} returned {} at {}",
                            name,
                            response.status(),
                            url
                        );
                    }
                    Err(e) => {
                        debug!("HTTP probe for {} failed: {}", name, e);
                        if e.is_connect() {
                            // Connection refused, process not listening yet
                        } else {
                            warn!("HTTP probe for {} unexpected error: {}", name, e);
                        }
                    }
                }

                tokio::time::sleep(period).await;
            }
        });
        Self { rx, task }
    }

    /// Wait for the HTTP probe to succeed.
    ///
    /// Returns `Some(())` when a 2xx response is received, or `None` if the
    /// probe task was cancelled.
    pub async fn recv(&mut self) -> Option<()> {
        self.rx.recv().await
    }
}

impl Drop for HttpGetProbe {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spawn a minimal HTTP server that responds to the first `num_503` requests
    /// with 503 and then responds with 200.
    async fn spawn_test_server(num_503: usize) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{}", addr);

        let handle = tokio::spawn(async move {
            let mut served = 0usize;
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                // Read the request (we do not care about its contents)
                let mut buf = [0u8; 1024];
                let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;

                let response = if served < num_503 {
                    "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n"
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"
                };
                let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
                served += 1;
            }
        });

        (url, handle)
    }

    #[tokio::test]
    async fn test_http_probe_succeeds_on_200() {
        let (url, _server) = spawn_test_server(0).await;

        let mut probe = HttpGetProbe::spawn(
            url,
            "test".to_string(),
            Duration::ZERO,
            Duration::from_millis(50),
            Duration::from_secs(5),
        );

        assert!(probe.recv().await.is_some());
    }

    #[tokio::test]
    async fn test_http_probe_retries_on_non_2xx() {
        // Server returns 503 for the first 2 requests, then 200
        let (url, _server) = spawn_test_server(2).await;

        let mut probe = HttpGetProbe::spawn(
            url,
            "test".to_string(),
            Duration::ZERO,
            Duration::from_millis(50),
            Duration::from_secs(5),
        );

        assert!(probe.recv().await.is_some());
    }
}
