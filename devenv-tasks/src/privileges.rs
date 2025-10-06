use nix::unistd::{Gid, Uid, geteuid, setgid, setuid};
use secrecy::{ExposeSecret, SecretString};
use std::env;

#[cfg(not(target_os = "macos"))]
use {nix::unistd::initgroups, std::ffi::CString};

/// Context information about the original user when running under sudo
#[derive(Debug, Clone)]
pub struct SudoContext {
    pub user: String,
    pub uid: Uid,
    pub gid: Gid,
    password: SecretString,
}

impl SudoContext {
    /// Get the password as bytes (exposes secret - use carefully)
    pub fn password_bytes(&self) -> &[u8] {
        self.password.expose_secret().as_bytes()
    }

    /// Detect if we're running under sudo and extract the original user context
    pub fn detect() -> Option<Self> {
        // Only if we're running as root AND have SUDO_USER set
        if !geteuid().is_root() {
            return None;
        }

        let user = env::var("SUDO_USER").ok()?;
        let uid = env::var("SUDO_UID").ok()?.parse().ok()?;
        let gid = env::var("SUDO_GID").ok()?.parse().ok()?;

        use std::io::{self, BufRead, BufReader};
        tracing::info!("Waiting for password");
        let mut reader = BufReader::new(io::stdin());
        let mut password = String::new();
        reader.read_line(&mut password).unwrap();

        Some(SudoContext {
            user,
            uid: Uid::from_raw(uid),
            gid: Gid::from_raw(gid),
            password: SecretString::new(password.into_boxed_str()),
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
