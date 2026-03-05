use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, info};

/// TCP readiness probe that connects in a loop until a connection succeeds.
///
/// Spawns a background task that polls one or more TCP addresses. Call `recv()`
/// to wait for the probe to succeed. Drop the probe to cancel the background task.
pub struct TcpProbe {
    rx: mpsc::Receiver<()>,
    task: JoinHandle<()>,
}

impl TcpProbe {
    /// Spawn a new TCP probe that polls `addresses` until any connection succeeds.
    ///
    /// When multiple addresses are given they are raced concurrently each cycle,
    /// so the probe succeeds as soon as any one of them accepts a connection.
    pub fn spawn(addresses: Vec<String>, name: String) -> Self {
        let (tx, rx) = mpsc::channel::<()>(1);
        let task = tokio::spawn(async move {
            debug!("Starting TCP probe for {} at {:?}", name, addresses);
            loop {
                let connected = match addresses.as_slice() {
                    [a, b] => {
                        tokio::select! {
                            Ok(_) = tokio::net::TcpStream::connect(a.as_str()) => true,
                            Ok(_) = tokio::net::TcpStream::connect(b.as_str()) => true,
                            else => false,
                        }
                    }
                    [a] => tokio::net::TcpStream::connect(a.as_str()).await.is_ok(),
                    _ => false,
                };
                if connected {
                    info!("TCP probe succeeded for {}", name);
                    let _ = tx.send(()).await;
                    return;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        Self { rx, task }
    }

    /// Wait for the TCP probe to succeed.
    ///
    /// Returns `Some(())` when the connection succeeds, or `None` if the
    /// probe task was cancelled.
    pub async fn recv(&mut self) -> Option<()> {
        self.rx.recv().await
    }
}

impl Drop for TcpProbe {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tcp_probe_succeeds_when_port_is_open() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();

        let mut probe = TcpProbe::spawn(vec![addr], "test".to_string());
        assert!(probe.recv().await.is_some());
    }

    #[tokio::test]
    async fn test_tcp_probe_succeeds_on_ipv6_only_listener() {
        // Bind on IPv6 loopback only; pass both IPv4 and IPv6 addresses to
        // verify the concurrent probe picks up [::1] even though 127.0.0.1 fails.
        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let mut probe = TcpProbe::spawn(
            vec![format!("127.0.0.1:{}", port), format!("[::1]:{}", port)],
            "test-ipv6".to_string(),
        );
        assert!(probe.recv().await.is_some());
    }
}
