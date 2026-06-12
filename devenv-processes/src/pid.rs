//! PID file utilities shared by process manager implementations.

use miette::{IntoDiagnostic, Result};
use nix::sys::signal;
use nix::unistd::Pid;
use std::path::{Path, PathBuf};
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
/// Removes stale PID files automatically (together with the mode marker, so a
/// crashed session's marker never misclassifies the next manager).
pub async fn check_pid_file(pid_file: &Path) -> Result<PidStatus> {
    let pid_str = match fs::read_to_string(pid_file).await {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PidStatus::NotFound);
        }
        Err(e) => {
            warn!("Unreadable PID file {}: {}", pid_file.display(), e);
            let _ = fs::remove_file(pid_file).await;
            remove_manager_mode(pid_file).await;
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
            remove_manager_mode(pid_file).await;
            return Ok(PidStatus::StaleRemoved);
        }
    };

    let pid = Pid::from_raw(pid_num);

    match signal::kill(pid, None) {
        Ok(_) => Ok(PidStatus::Running(pid)),
        Err(nix::errno::Errno::ESRCH) => {
            warn!("Stale PID {} in {}, removing", pid, pid_file.display());
            let _ = fs::remove_file(pid_file).await;
            remove_manager_mode(pid_file).await;
            Ok(PidStatus::StaleRemoved)
        }
        Err(e) => {
            warn!("Error checking PID {}: {}, removing file", pid, e);
            let _ = fs::remove_file(pid_file).await;
            remove_manager_mode(pid_file).await;
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

/// How the running native manager session was started. Stored in a sibling
/// file of the pid file because the pid file format is a bare integer that
/// older readers parse strictly (and delete on mismatch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerMode {
    /// An interactive `devenv up` (or an in-process detached manager such as
    /// `devenv test`) owns the session from a live devenv process.
    Foreground,
    /// A detached daemon spawned by `devenv up -d` owns the session.
    Daemon,
}

/// Path of the mode marker: the pid file with extension `mode`.
pub fn manager_mode_file(pid_file: &Path) -> PathBuf {
    pid_file.with_extension("mode")
}

/// Best-effort write of the mode marker next to the pid file.
///
/// Written before the pid file so that a reader who sees the pid as running
/// never observes a missing marker for a session that has one.
pub async fn write_manager_mode(pid_file: &Path, mode: ManagerMode) {
    let contents = match mode {
        ManagerMode::Foreground => "foreground",
        ManagerMode::Daemon => "daemon",
    };
    if let Err(e) = fs::write(manager_mode_file(pid_file), contents).await {
        warn!(error = %e, path = %pid_file.display(), "failed to write manager mode marker");
    }
}

/// Read the mode marker; `None` when missing or unrecognized. Callers treat
/// `None` as `Daemon` for compatibility with managers started by older devenv
/// versions that wrote no marker.
pub async fn read_manager_mode(pid_file: &Path) -> Option<ManagerMode> {
    let contents = fs::read_to_string(manager_mode_file(pid_file)).await.ok()?;
    match contents.trim() {
        "foreground" => Some(ManagerMode::Foreground),
        "daemon" => Some(ManagerMode::Daemon),
        _ => None,
    }
}

/// Best-effort removal, paired with pid-file removal everywhere so stale
/// markers from crashed sessions cannot misclassify a later manager.
pub async fn remove_manager_mode(pid_file: &Path) {
    let _ = fs::remove_file(manager_mode_file(pid_file)).await;
}
