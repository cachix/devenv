//! PTY (pseudo-terminal) management.
//!
//! Provides a thread-safe PTY wrapper that handles spawning shell processes,
//! reading/writing data, and managing the PTY lifecycle.

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{self, Read, Write};
use std::sync::Mutex;
use thiserror::Error;

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
        let master = self.master.lock().unwrap();
        master
            .resize(size)
            .map_err(|e| PtyError::Resize(e.to_string()))
    }

    /// Try to wait for the child process without blocking.
    pub fn try_wait(&self) -> Result<Option<portable_pty::ExitStatus>, PtyError> {
        let mut child = self.child.lock().unwrap();
        child
            .try_wait()
            .map_err(|e| PtyError::Io(io::Error::other(e.to_string())))
    }

    /// Kill the PTY child process.
    pub fn kill(&self) -> Result<(), PtyError> {
        let mut child = self.child.lock().unwrap();
        child
            .kill()
            .map_err(|e| PtyError::Io(io::Error::other(e.to_string())))
    }
}

/// Get the current terminal size.
pub fn get_terminal_size() -> PtySize {
    if let Some((cols, rows)) = term_size::dimensions() {
        PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        }
    } else {
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }
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
        let io_err = io::Error::new(io::ErrorKind::Other, "test");
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
