use nix::unistd::{Gid, Uid, geteuid, setgid, setuid};
use std::env;

#[cfg(not(target_os = "macos"))]
use {nix::unistd::initgroups, std::ffi::CString};

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
        #[cfg(not(target_os = "macos"))]
        {
            // Fetch and set the supplementary group access list (not available on macOS)
            let username = CString::new(self.user.as_str()).map_err(|_| nix::Error::EINVAL)?;
            initgroups(&username, self.gid)?;
        }

        setgid(self.gid)?;
        setuid(self.uid)?;

        Ok(())
    }
}
