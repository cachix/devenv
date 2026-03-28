use std::collections::HashMap;
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
        bash: String,
        env: HashMap<String, String>,
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
                let mut cmd = tokio::process::Command::new(&bash);
                cmd.arg("-c")
                    .arg(&command)
                    .envs(&env)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());

                #[cfg(unix)]
                {
                    cmd.process_group(0);
                }

                let mut child = match cmd.spawn() {
                    Ok(child) => child,
                    Err(e) => {
                        warn!("Exec probe for {} failed to run: {}", name, e);
                        tokio::time::sleep(period).await;
                        continue;
                    }
                };

                let result = tokio::time::timeout(timeout, child.wait()).await;

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
                        warn!("Exec probe for {} failed to wait: {}", name, e);
                    }
                    Err(_) => {
                        debug!("Exec probe for {} timed out, killing process group", name);
                        #[cfg(unix)]
                        {
                            if let Some(pid) = child.id() {
                                use nix::sys::signal::{Signal, kill};
                                use nix::unistd::Pid;

                                let _ = kill(Pid::from_raw(-(pid as i32)), Signal::SIGKILL);
                            }
                        }
                        let _ = child.kill().await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_exec_probe_succeeds_on_exit_zero() {
        let mut probe = ExecProbe::spawn(
            "exit 0".to_string(),
            "test".to_string(),
            "bash".to_string(),
            HashMap::new(),
            Duration::ZERO,
            Duration::from_millis(50),
            Duration::from_secs(5),
        );

        assert!(probe.recv().await.is_some());
    }

    #[tokio::test]
    async fn test_exec_probe_retries_until_success() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("gate");

        // Probe checks for a file that does not exist yet
        let cmd = format!("test -f {}", marker.display());

        let mut probe = ExecProbe::spawn(
            cmd,
            "test-retry".to_string(),
            "bash".to_string(),
            HashMap::new(),
            Duration::ZERO,
            Duration::from_millis(50),
            Duration::from_secs(5),
        );

        // Create the marker file so the next probe attempt succeeds
        tokio::fs::write(&marker, "").await.unwrap();

        assert!(probe.recv().await.is_some());
    }
}
