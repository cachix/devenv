//! PID file utilities shared by process manager implementations.

use miette::{IntoDiagnostic, Result};
use nix::sys::signal;
use nix::unistd::Pid;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio::fs;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
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

/// Write PID to a file
pub async fn write_pid(pid_file: &Path, pid: u32) -> Result<()> {
    fs::write(pid_file, pid.to_string()).await.into_diagnostic()
}

/// How often a long-running manager refreshes its PID file.
///
/// macOS runs `/usr/libexec/tmp_cleaner` nightly, which deletes regular files
/// under `/tmp` whose atime, mtime and ctime are all older than three days.
/// The process runtime directory usually lives there, so a manager that runs
/// for days would otherwise lose its PID file and become unreachable.
pub const PID_FILE_REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Bump the PID file's modification time, or rewrite it if it is missing or
/// no longer holds `pid`.
///
/// An up-to-date file is only touched, never truncated, so a concurrent
/// [`check_pid_file`] cannot observe an empty file and remove it as invalid.
pub async fn refresh_pid(pid_file: &Path, pid: u32) -> Result<()> {
    match fs::read_to_string(pid_file).await {
        Ok(content) if content.trim() == pid.to_string() => {
            let file = std::fs::File::options()
                .write(true)
                .open(pid_file)
                .into_diagnostic()?;
            file.set_modified(SystemTime::now()).into_diagnostic()
        }
        _ => write_pid(pid_file, pid).await,
    }
}

/// Background task that keeps a PID file alive while its owner runs.
///
/// The task stops when [`PidFileRefresher::stop`] is called or the refresher
/// is dropped.
pub struct PidFileRefresher {
    handle: Option<JoinHandle<()>>,
}

impl PidFileRefresher {
    /// Refresh `pid_file` every `interval`, starting one interval from now.
    pub fn spawn(pid_file: PathBuf, pid: u32, interval: Duration) -> Self {
        let handle = tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                if let Err(e) = refresh_pid(&pid_file, pid).await {
                    warn!("Failed to refresh PID file {}: {}", pid_file.display(), e);
                }
            }
        });
        Self {
            handle: Some(handle),
        }
    }

    /// Stop refreshing and wait for the task to finish, so a following
    /// removal of the PID file cannot be undone by an in-flight refresh.
    pub async fn stop(mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
            let _ = handle.await;
        }
    }
}

impl Drop for PidFileRefresher {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

/// Remove a PID file
pub async fn remove_pid(pid_file: &Path) -> Result<()> {
    fs::remove_file(pid_file).await.into_diagnostic()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refresh_pid_touches_matching_file() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("manager.pid");
        write_pid(&pid_file, 4242).await.unwrap();
        let old = SystemTime::now() - Duration::from_secs(4 * 24 * 60 * 60);
        std::fs::File::options()
            .write(true)
            .open(&pid_file)
            .unwrap()
            .set_modified(old)
            .unwrap();

        refresh_pid(&pid_file, 4242).await.unwrap();

        let modified = std::fs::metadata(&pid_file).unwrap().modified().unwrap();
        assert!(modified > old + Duration::from_secs(24 * 60 * 60));
        assert_eq!(std::fs::read_to_string(&pid_file).unwrap(), "4242");
    }

    #[tokio::test]
    async fn refresh_pid_rewrites_missing_or_mismatched_file() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("manager.pid");

        refresh_pid(&pid_file, 4242).await.unwrap();
        assert_eq!(std::fs::read_to_string(&pid_file).unwrap(), "4242");

        std::fs::write(&pid_file, "1").unwrap();
        refresh_pid(&pid_file, 4242).await.unwrap();
        assert_eq!(std::fs::read_to_string(&pid_file).unwrap(), "4242");
    }

    #[tokio::test]
    async fn refresher_recreates_deleted_file_until_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("manager.pid");
        write_pid(&pid_file, 4242).await.unwrap();

        let refresher = PidFileRefresher::spawn(pid_file.clone(), 4242, Duration::from_millis(20));
        std::fs::remove_file(&pid_file).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !pid_file.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "PID file was not recreated"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(std::fs::read_to_string(&pid_file).unwrap(), "4242");

        refresher.stop().await;
        std::fs::remove_file(&pid_file).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!pid_file.exists());
    }
}
