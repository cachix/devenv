//! Terminal utilities.
//!
//! Provides utilities for managing terminal state, including raw mode handling.

use std::io;

/// Check if stdin is a TTY.
pub fn is_tty() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = io::stdin().as_raw_fd();
        let result = unsafe { libc::isatty(fd) };
        result == 1
    }

    #[cfg(not(unix))]
    {
        false
    }
}

/// Raw terminal mode guard that restores terminal state on drop.
///
/// When created, this guard puts the terminal into raw mode. When dropped,
/// it restores the original terminal settings. This ensures proper cleanup
/// even if the program panics or exits early.
pub struct RawModeGuard {
    #[cfg(unix)]
    original: Option<libc::termios>,
}

impl RawModeGuard {
    /// Enter raw mode. Returns a guard that restores settings on drop.
    ///
    /// If stdin is not a TTY (e.g., in CI or tests), this is a no-op.
    pub fn new() -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = io::stdin().as_raw_fd();

            // Skip raw mode if stdin is not a terminal (e.g., in CI or tests)
            if unsafe { libc::isatty(fd) } == 0 {
                return Ok(Self { original: None });
            }

            let mut termios: libc::termios = unsafe { std::mem::zeroed() };
            if unsafe { libc::tcgetattr(fd, &mut termios) } != 0 {
                return Err(io::Error::last_os_error());
            }
            let original = termios;

            unsafe { libc::cfmakeraw(&mut termios) };
            if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &termios) } != 0 {
                return Err(io::Error::last_os_error());
            }

            Ok(Self {
                original: Some(original),
            })
        }

        #[cfg(not(unix))]
        Ok(Self {})
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(original) = self.original {
            use std::os::unix::io::AsRawFd;
            let fd = io::stdin().as_raw_fd();
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &original) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_tty_returns_bool() {
        // Just verify it doesn't panic and returns a bool
        let _ = is_tty();
    }

    #[test]
    fn test_raw_mode_guard_creation() {
        // In test environment, this should succeed even if not a TTY
        let guard = RawModeGuard::new();
        assert!(guard.is_ok());
    }
}
