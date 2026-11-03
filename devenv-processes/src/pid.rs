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
    let pid_str = match fs::read_to_string(pid_file).await {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PidStatus::NotFound);
        }
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

/// Publish a complete PID atomically so concurrent validators cannot remove a
/// partially written file as stale. The staging file must share its filesystem.
pub async fn write_pid(pid_file: &Path, pid: u32) -> Result<()> {
    let pid_file = pid_file.to_path_buf();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        use std::io::Write;

        let parent = pid_file
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        write!(staged, "{pid}")?;
        staged.persist(pid_file).map_err(|error| error.error)?;
        Ok(())
    })
    .await
    .into_diagnostic()?
    .into_diagnostic()
}

/// Remove a PID file
pub async fn remove_pid(pid_file: &Path) -> Result<()> {
    fs::remove_file(pid_file).await.into_diagnostic()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn failed_pid_publication_removes_its_staging_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manager.pid");
        fs::create_dir(&path).await.unwrap();
        assert!(write_pid(&path, std::process::id()).await.is_err());
        assert!(path.is_dir());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_pid_checks_never_observe_partial_publication() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manager.pid");
        let pid = std::process::id();
        write_pid(&path, pid).await.unwrap();
        let barrier = Arc::new(tokio::sync::Barrier::new(5));
        let writer = {
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            tokio::spawn(async move {
                barrier.wait().await;
                for _ in 0..512 {
                    write_pid(&path, pid).await.unwrap();
                }
            })
        };
        let mut readers = Vec::new();
        for _ in 0..4 {
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            readers.push(tokio::spawn(async move {
                barrier.wait().await;
                for _ in 0..512 {
                    let status = check_pid_file(&path).await.unwrap();
                    if !matches!(status, PidStatus::Running(live) if live.as_raw() == pid as i32) {
                        return Err(format!("Reader observed {status:?} during publication"));
                    }
                }
                Ok(())
            }));
        }
        writer.await.unwrap();
        for reader in readers {
            reader.await.unwrap().unwrap();
        }
        assert!(matches!(
            check_pid_file(&path).await.unwrap(),
            PidStatus::Running(_)
        ));
    }
}
