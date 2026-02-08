//! PID file utilities shared by process manager implementations.

use miette::{IntoDiagnostic, Result};
use nix::sys::signal;
use nix::unistd::Pid;
use std::path::Path;
use tokio::fs;
use tracing::warn;

/// Result of validating a PID file
#[derive(Debug)]
pub enum PidStatus {
    /// Process is running with the given PID
    Running(Pid),
    /// PID file doesn't exist
    NotFound,
    /// PID file was stale and has been removed
    StaleRemoved,
}

/// Check if a PID file exists and the process is still running.
/// Removes stale PID files automatically.
pub async fn check_pid_file(pid_file: &Path) -> Result<PidStatus> {
    if !pid_file.exists() {
        return Ok(PidStatus::NotFound);
    }

    let pid_str = match fs::read_to_string(pid_file).await {
        Ok(s) => s,
        Err(e) => {
            warn!("Unreadable PID file {}: {}", pid_file.display(), e);
            let _ = fs::remove_file(pid_file).await;
            return Ok(PidStatus::StaleRemoved);
        }
    };

    let pid_num = match pid_str.trim().parse::<i32>() {
        Ok(p) => p,
        Err(_) => {
            warn!(
                "Invalid PID format in {}: '{}'",
                pid_file.display(),
                pid_str.trim()
            );
            let _ = fs::remove_file(pid_file).await;
            return Ok(PidStatus::StaleRemoved);
        }
    };

    let pid = Pid::from_raw(pid_num);

    match signal::kill(pid, None) {
        Ok(_) => Ok(PidStatus::Running(pid)),
        Err(nix::errno::Errno::ESRCH) => {
            warn!("Stale PID {} in {}, removing", pid, pid_file.display());
            let _ = fs::remove_file(pid_file).await;
            Ok(PidStatus::StaleRemoved)
        }
        Err(e) => {
            warn!("Error checking PID {}: {}, removing file", pid, e);
            let _ = fs::remove_file(pid_file).await;
            Ok(PidStatus::StaleRemoved)
        }
    }
}

/// Read PID from a file
pub async fn read_pid(pid_file: &Path) -> Result<Pid> {
    let content = fs::read_to_string(pid_file).await.into_diagnostic()?;
    let pid_num = content.trim().parse::<i32>().into_diagnostic()?;
    Ok(Pid::from_raw(pid_num))
}

/// Write PID to a file
pub async fn write_pid(pid_file: &Path, pid: u32) -> Result<()> {
    fs::write(pid_file, pid.to_string()).await.into_diagnostic()
}

/// Remove a PID file
pub async fn remove_pid(pid_file: &Path) -> Result<()> {
    fs::remove_file(pid_file).await.into_diagnostic()
}
