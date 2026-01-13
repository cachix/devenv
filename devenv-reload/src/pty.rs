use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PtyError {
    #[error("failed to create PTY: {0}")]
    Create(#[source] anyhow::Error),
    #[error("failed to spawn command: {0}")]
    Spawn(#[source] anyhow::Error),
    #[error("failed to clone reader: {0}")]
    CloneReader(#[source] anyhow::Error),
    #[error("failed to get writer: {0}")]
    Writer(#[source] anyhow::Error),
    #[error("failed to resize PTY: {0}")]
    Resize(#[source] anyhow::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
}

impl Pty {
    pub fn spawn(cmd: CommandBuilder, size: PtySize) -> Result<Self, PtyError> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(size).map_err(PtyError::Create)?;

        let child = pair.slave.spawn_command(cmd).map_err(PtyError::Spawn)?;

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(PtyError::CloneReader)?;
        let writer = pair.master.take_writer().map_err(PtyError::Writer)?;

        Ok(Self {
            master: pair.master,
            child,
            reader,
            writer,
        })
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, PtyError> {
        Ok(self.reader.read(buf)?)
    }

    pub fn write_all(&mut self, data: &[u8]) -> Result<(), PtyError> {
        Ok(self.writer.write_all(data)?)
    }

    pub fn flush(&mut self) -> Result<(), PtyError> {
        Ok(self.writer.flush()?)
    }

    pub fn resize(&self, size: PtySize) -> Result<(), PtyError> {
        self.master.resize(size).map_err(PtyError::Resize)
    }

    pub fn try_wait(&mut self) -> Result<Option<portable_pty::ExitStatus>, PtyError> {
        self.child
            .try_wait()
            .map_err(|e| PtyError::Io(std::io::Error::other(e.to_string())))
    }

    pub fn kill(&mut self) -> Result<(), PtyError> {
        self.child
            .kill()
            .map_err(|e| PtyError::Io(std::io::Error::other(e.to_string())))
    }
}

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
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let pty_err: PtyError = io_err.into();
        assert!(matches!(pty_err, PtyError::Io(_)));
    }

    #[test]
    fn test_pty_error_display() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "test");
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
