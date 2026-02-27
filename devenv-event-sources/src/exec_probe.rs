use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// Exec readiness probe that runs a shell command in a loop until it exits 0.
///
/// Spawns a background task that periodically executes a command. Call `recv()`
/// to wait for the probe to succeed. Drop the probe to cancel the background task.
pub struct ExecProbe {
    rx: mpsc::Receiver<()>,
    task: JoinHandle<()>,
}

impl ExecProbe {
    /// Spawn a new exec probe that runs `command` (via `sh -c`) every `period`
    /// seconds, with an `initial_delay` before the first attempt and a per-attempt
    /// `timeout`.
    pub fn spawn(
        command: String,
        name: String,
        initial_delay: Duration,
        period: Duration,
        timeout: Duration,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<()>(1);
        let task = tokio::spawn(async move {
            debug!("Starting exec probe for {}: {}", name, command);

            if !initial_delay.is_zero() {
                tokio::time::sleep(initial_delay).await;
            }

            loop {
                let result = tokio::time::timeout(
                    timeout,
                    tokio::process::Command::new("sh")
                        .arg("-c")
                        .arg(&command)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status(),
                )
                .await;

                match result {
                    Ok(Ok(status)) if status.success() => {
                        info!("Exec probe succeeded for {}", name);
                        let _ = tx.send(()).await;
                        break;
                    }
                    Ok(Ok(status)) => {
                        debug!("Exec probe for {} exited with {}", name, status);
                    }
                    Ok(Err(e)) => {
                        warn!("Exec probe for {} failed to run: {}", name, e);
                    }
                    Err(_) => {
                        debug!("Exec probe for {} timed out", name);
                    }
                }

                tokio::time::sleep(period).await;
            }
        });
        Self { rx, task }
    }

    /// Wait for the exec probe to succeed.
    ///
    /// Returns `Some(())` when the command exits 0, or `None` if the
    /// probe task was cancelled.
    pub async fn recv(&mut self) -> Option<()> {
        self.rx.recv().await
    }
}

impl Drop for ExecProbe {
    fn drop(&mut self) {
        self.task.abort();
    }
}
