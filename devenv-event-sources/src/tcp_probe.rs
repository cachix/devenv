use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, info};

/// TCP readiness probe that connects in a loop until the port is reachable.
///
/// Spawns a background task that polls a TCP address. Call `recv()` to wait
/// for the probe to succeed. Drop the probe to cancel the background task.
pub struct TcpProbe {
    rx: mpsc::Receiver<()>,
    task: JoinHandle<()>,
}

impl TcpProbe {
    /// Spawn a new TCP probe that polls `address` until a connection succeeds.
    pub fn spawn(address: String, name: String) -> Self {
        let (tx, rx) = mpsc::channel::<()>(1);
        let task = tokio::spawn(async move {
            debug!("Starting TCP probe for {} at {}", name, address);
            loop {
                match tokio::net::TcpStream::connect(&address).await {
                    Ok(_) => {
                        info!("TCP probe succeeded for {} at {}", name, address);
                        let _ = tx.send(()).await;
                        break;
                    }
                    Err(_) => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
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
