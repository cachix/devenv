//! PTY (pseudo-terminal) management.
//!
//! Provides a thread-safe PTY wrapper that handles spawning shell processes,
//! reading/writing data, and managing the PTY lifecycle.

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{self, Read, Write};
use std::sync::Mutex;
use std::time::Duration;
use thiserror::Error;

/// Retry budget for collecting the child's exit status. The status lands on the
/// first attempt when idle and by the fourth under load; the rest is give-up
/// margin.
const REAP_ATTEMPTS: u32 = 1000;
const REAP_POLL_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Debug, Error)]
pub enum PtyError {
    #[error("failed to create PTY: {0}")]
    Create(String),
    #[error("failed to spawn command: {0}")]
    Spawn(String),
    #[error("failed to clone reader: {0}")]
    CloneReader(String),
    #[error("failed to get writer: {0}")]
    Writer(String),
    #[error("failed to resize PTY: {0}")]
    Resize(String),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

/// PTY wrapper with separate read/write locks.
///
/// Reader and writer are protected by separate locks to avoid blocking
/// input writes while a blocking read is in progress.
pub struct Pty {
    master: Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    reader: Mutex<Box<dyn Read + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
}

impl Pty {
    /// Spawn a new PTY with the given command and size.
    pub fn spawn(cmd: CommandBuilder, size: PtySize) -> Result<Self, PtyError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(size)
            .map_err(|e| PtyError::Create(e.to_string()))?;

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| PtyError::Spawn(e.to_string()))?;

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::CloneReader(e.to_string()))?;

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| PtyError::Writer(e.to_string()))?;

        Ok(Self {
            master: Mutex::new(pair.master),
            child: Mutex::new(child),
            reader: Mutex::new(reader),
            writer: Mutex::new(writer),
        })
    }

    /// Read from the PTY.
    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let mut reader = self.reader.lock().unwrap();
        reader.read(buf)
    }

    /// Write data to the PTY.
    pub fn write_all(&self, data: &[u8]) -> io::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(data)
    }

    /// Flush the PTY writer.
    pub fn flush(&self) -> io::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.flush()
    }

    /// Resize the PTY.
    pub fn resize(&self, size: PtySize) -> Result<(), PtyError> {
        let master = self.master.lock().unwrap_or_else(|e| e.into_inner());
        master
            .resize(size)
            .map_err(|e| PtyError::Resize(e.to_string()))
    }

    /// Try to wait for the child process without blocking.
    pub fn try_wait(&self) -> Result<Option<portable_pty::ExitStatus>, PtyError> {
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        child
            .try_wait()
            .map_err(|e| PtyError::Io(io::Error::other(e.to_string())))
    }

    /// Collect the child's exit status once the PTY has closed.
    ///
    /// The kernel closes the child's descriptors before making it reapable, so
    /// the reader observes the PTY closing while `waitpid` still reports the
    /// child running, and a single [`Self::try_wait`] loses the exit code — on
    /// Linux roughly half the time under load. Retrying is bounded rather than a
    /// blocking `Child::wait` because the reader also arrives here when a live
    /// child merely closed the PTY (a read error on Linux), where waiting would
    /// hold the child lock that [`Self::kill`] needs to end it.
    ///
    /// This polls because the status is collected from the wrong event.
    /// Decoupling the two would have the status delivered rather than waited
    /// for: reap on SIGCHLD, hand the child to a thread that blocks in `wait`,
    /// or watch a pidfd or kqueue. Doing that here means moving the child out
    /// from behind this lock so `kill` can use a pid-only `clone_killer`
    /// signaller instead, which reworks the shutdown path.
    pub fn wait_for_exit(&self) -> Option<portable_pty::ExitStatus> {
        for _ in 0..REAP_ATTEMPTS {
            match self.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) => std::thread::sleep(REAP_POLL_INTERVAL),
                Err(e) => {
                    tracing::debug!(error = %e, "failed to reap inner shell");
                    return None;
                }
            }
        }
        tracing::debug!("inner shell unreaped after PTY close, exit code unavailable");
        None
    }

    /// Kill the PTY child process. Recovers from a poisoned mutex.
    pub fn kill(&self) -> Result<(), PtyError> {
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        child
            .kill()
            .map_err(|e| PtyError::Io(io::Error::other(e.to_string())))
    }
}

/// Get the current terminal size.
///
/// `crossterm::terminal::size()` reads the size via `TIOCGWINSZ` on the
/// controlling terminal. Under WSL2, that ioctl can succeed with a `0x0`
/// winsize for a brief window right as a new process attaches to the pty
/// (e.g. `devenv`'s zsh/bash hook auto-spawning `devenv shell` on `cd`) —
/// the Windows-side pty host hasn't reported real dimensions yet, but
/// crossterm has no way to tell that apart from a legitimately empty
/// terminal, so it returns `Ok((0, 0))` rather than an `Err` we could
/// already fall back on below.
///
/// A `0` in either dimension is never usable: it reaches
/// `libghostty_vt::Terminal::new(cols, rows)` when the interactive shell
/// session starts its VT emulator, and that rejects zero dimensions
/// outright with `Error::InvalidValue` ("terminal error: invalid value"),
/// aborting the whole session. Treat a zero-sized `Ok` result the same as
/// a failed query and fall back to the default.
pub fn get_terminal_size() -> PtySize {
    const DEFAULT_SIZE: (u16, u16) = (80, 24);
    let (cols, rows) = match crossterm::terminal::size() {
        Ok((cols, rows)) if cols != 0 && rows != 0 => (cols, rows),
        _ => DEFAULT_SIZE,
    };
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_error_io_from() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "not found");
        let pty_err: PtyError = io_err.into();
        assert!(matches!(pty_err, PtyError::Io(_)));
    }

    #[test]
    fn test_pty_error_display() {
        let io_err = io::Error::other("test");
        let pty_err = PtyError::Io(io_err);
        let display = format!("{}", pty_err);
        assert!(display.contains("IO error"));
    }

    #[test]
    fn test_get_terminal_size_returns_valid_size() {
        let size = get_terminal_size();
        // Should return either actual size or default 80x24
        assert!(size.cols >= 1);
        assert!(size.rows >= 1);
    }
}
