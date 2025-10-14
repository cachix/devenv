//! Systemd-style notify socket for process health monitoring.
//!
//! Implements the sd_notify protocol allowing processes to send:
//! - READY=1: Process is ready to serve
//! - WATCHDOG=1: Heartbeat ping
//! - STATUS=...: Human-readable status
//! - STOPPING=1: Process is shutting down

use miette::{IntoDiagnostic, Result, WrapErr};
use std::path::{Path, PathBuf};
use tokio::net::UnixDatagram;
use tracing::debug;

/// Messages that can be received via the notify socket
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyMessage {
    /// Process signals it's ready to serve (READY=1)
    Ready,
    /// Process is shutting down (STOPPING=1)
    Stopping,
    /// Process is reloading configuration (RELOADING=1)
    Reloading,
    /// Watchdog heartbeat (WATCHDOG=1)
    Watchdog,
    /// Human-readable status message (STATUS=...)
    Status(String),
    /// Unrecognized message
    Unknown(String),
}

/// Unix datagram socket for receiving systemd-style notifications
pub struct NotifySocket {
    socket: UnixDatagram,
    path: PathBuf,
}

impl NotifySocket {
    /// Create a new notify socket for a process
    ///
    /// Creates a Unix datagram socket at `state_dir/notify/<process_name>.sock`
    pub async fn new(state_dir: &Path, process_name: &str) -> Result<Self> {
        let notify_dir = state_dir.join("notify");
        std::fs::create_dir_all(&notify_dir)
            .into_diagnostic()
            .wrap_err("Failed to create notify directory")?;

        let path = notify_dir.join(format!("{}.sock", process_name));

        // Remove existing socket if present
        if path.exists() {
            std::fs::remove_file(&path)
                .into_diagnostic()
                .wrap_err("Failed to remove existing notify socket")?;
        }

        let socket = UnixDatagram::bind(&path)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to bind notify socket at {}", path.display()))?;

        debug!("Created notify socket at {}", path.display());

        Ok(Self { socket, path })
    }

    /// Get the path to the notify socket
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Receive a message from the notify socket
    ///
    /// Returns a vector of parsed messages (a single datagram can contain multiple)
    pub async fn recv(&self) -> Result<Vec<NotifyMessage>> {
        let mut buf = vec![0u8; 4096];
        let len = self
            .socket
            .recv(&mut buf)
            .await
            .into_diagnostic()
            .wrap_err("Failed to receive from notify socket")?;

        let data = String::from_utf8_lossy(&buf[..len]);
        Ok(parse_notify_message(&data))
    }
}

impl Drop for NotifySocket {
    fn drop(&mut self) {
        // Clean up the socket file
        if self.path.exists() {
            let _ = std::fs::remove_file(&self.path);
            debug!("Removed notify socket at {}", self.path.display());
        }
    }
}

/// Parse systemd notify message format
///
/// Format: KEY=value pairs separated by newlines
fn parse_notify_message(data: &str) -> Vec<NotifyMessage> {
    let mut messages = Vec::new();

    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let msg = match key {
                "READY" if value == "1" => NotifyMessage::Ready,
                "STOPPING" if value == "1" => NotifyMessage::Stopping,
                "RELOADING" if value == "1" => NotifyMessage::Reloading,
                "WATCHDOG" if value == "1" => NotifyMessage::Watchdog,
                "STATUS" => NotifyMessage::Status(value.to_string()),
                _ => NotifyMessage::Unknown(line.to_string()),
            };
            messages.push(msg);
        } else {
            messages.push(NotifyMessage::Unknown(line.to_string()));
        }
    }

    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ready() {
        let msgs = parse_notify_message("READY=1\n");
        assert_eq!(msgs, vec![NotifyMessage::Ready]);
    }

    #[test]
    fn test_parse_watchdog() {
        let msgs = parse_notify_message("WATCHDOG=1\n");
        assert_eq!(msgs, vec![NotifyMessage::Watchdog]);
    }

    #[test]
    fn test_parse_status() {
        let msgs = parse_notify_message("STATUS=Loading configuration...\n");
        assert_eq!(
            msgs,
            vec![NotifyMessage::Status(
                "Loading configuration...".to_string()
            )]
        );
    }

    #[test]
    fn test_parse_multiple() {
        let msgs = parse_notify_message("READY=1\nSTATUS=Ready to serve\n");
        assert_eq!(
            msgs,
            vec![
                NotifyMessage::Ready,
                NotifyMessage::Status("Ready to serve".to_string())
            ]
        );
    }

    #[test]
    fn test_parse_stopping() {
        let msgs = parse_notify_message("STOPPING=1\n");
        assert_eq!(msgs, vec![NotifyMessage::Stopping]);
    }

    #[test]
    fn test_parse_unknown() {
        let msgs = parse_notify_message("CUSTOM=value\n");
        assert_eq!(
            msgs,
            vec![NotifyMessage::Unknown("CUSTOM=value".to_string())]
        );
    }

    #[tokio::test]
    async fn test_socket_creation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let socket = NotifySocket::new(temp_dir.path(), "test-process").await;
        assert!(socket.is_ok());

        let socket = socket.unwrap();
        assert!(socket.path().exists());
        assert!(
            socket
                .path()
                .to_string_lossy()
                .contains("test-process.sock")
        );
    }

    #[tokio::test]
    async fn test_socket_cleanup_on_drop() {
        let temp_dir = tempfile::tempdir().unwrap();
        let socket_path;
        {
            let socket = NotifySocket::new(temp_dir.path(), "cleanup-test")
                .await
                .unwrap();
            socket_path = socket.path().to_path_buf();
            assert!(socket_path.exists());
        }
        // Socket should be cleaned up after drop
        assert!(!socket_path.exists());
    }
}
