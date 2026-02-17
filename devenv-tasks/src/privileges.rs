use miette::{IntoDiagnostic, bail};
use nix::unistd::{Gid, Uid, geteuid, setgid, setuid};
use std::env;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Keeps a sudo credential refresh task alive for a scope.
/// The task is aborted when this guard is dropped.
pub struct SudoCredentialRefresh {
    handle: Option<JoinHandle<()>>,
}

impl SudoCredentialRefresh {
    fn new(handle: JoinHandle<()>) -> Self {
        Self {
            handle: Some(handle),
        }
    }
}

impl Drop for SudoCredentialRefresh {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

/// Context information about the original user when running under sudo
#[derive(Debug, Clone)]
pub struct SudoContext {
    pub user: String,
    pub uid: Uid,
    pub gid: Gid,
}

impl SudoContext {
    /// Detect if we're running under sudo and extract the original user context
    pub fn detect() -> Option<Self> {
        // Only if we're running as root AND have SUDO_USER set
        if !geteuid().is_root() {
            return None;
        }

        let user = env::var("SUDO_USER").ok()?;
        let uid = env::var("SUDO_UID").ok()?.parse().ok()?;
        let gid = env::var("SUDO_GID").ok()?.parse().ok()?;

        Some(SudoContext {
            user,
            uid: Uid::from_raw(uid),
            gid: Gid::from_raw(gid),
        })
    }

    /// Drop privileges to the original user
    ///
    /// Order matters: we must set GID first, then UID, because once we drop UID privileges we can't change GID anymore.
    pub fn drop_privileges(&self) -> Result<(), nix::Error> {
        setgid(self.gid)?;
        setuid(self.uid)?;
        Ok(())
    }
}

/// Check if sudo credentials are already cached (non-interactive).
/// Returns Ok(()) if cached, Err if not.
pub fn ensure_sudo_authenticated_noninteractive() -> miette::Result<()> {
    let status = std::process::Command::new("sudo")
        .args(["-n", "-v"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .into_diagnostic()?;

    if status.success() {
        Ok(())
    } else {
        bail!(
            "Tasks require sudo but credentials are not cached.\n\
             Run `sudo -v` first, or use `--no-tui` to allow an interactive password prompt."
        )
    }
}

/// Ensure sudo credentials are available, prompting for a password if needed.
/// First tries non-interactive check; if that fails, runs `sudo -v` with inherited
/// stdio so the user can type their password.
pub async fn ensure_sudo_authenticated() -> miette::Result<()> {
    // Try non-interactive first
    let status = tokio::process::Command::new("sudo")
        .args(["-n", "-v"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .into_diagnostic()?;

    if status.success() {
        return Ok(());
    }

    // Credentials not cached — prompt the user
    let status = tokio::process::Command::new("sudo")
        .arg("-v")
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .await
        .into_diagnostic()?;

    if status.success() {
        Ok(())
    } else {
        bail!("sudo authentication failed")
    }
}

/// Spawn a background task that refreshes sudo credentials periodically.
///
/// Uses a 60-second interval to be safe regardless of the system's `timestamp_timeout`
/// setting in sudoers (the default is typically 5 or 15 minutes, but can be shorter).
/// Stops when the cancellation token is triggered.
pub fn spawn_sudo_credential_refresh(cancel: CancellationToken) -> SudoCredentialRefresh {
    let handle = tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(60);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(interval) => {
                    match tokio::process::Command::new("sudo")
                        .args(["-n", "-v"])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .await
                    {
                        Ok(status) if !status.success() => {
                            tracing::warn!("Failed to refresh sudo credentials; tasks requiring sudo may fail");
                        }
                        Err(e) => {
                            tracing::warn!("Failed to run sudo credential refresh: {}", e);
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    SudoCredentialRefresh::new(handle)
}
