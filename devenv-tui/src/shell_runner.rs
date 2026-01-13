//! Shell runner for TUI shell mode.
//!
//! This module provides a shell runner that operates outside the iocraft TUI framework.
//! It handles PTY spawning, keyboard forwarding, and terminal output, with support for
//! hot-reload via the ShellCoordinator.

use avt::Vt;
use crossterm::{
    cursor, execute,
    style::{Color, Print, SetBackgroundColor, SetForegroundColor},
    terminal::{self, ClearType},
};
use devenv_reload::{ShellCommand, ShellEvent};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Error)]
pub enum ShellRunnerError {
    #[error("PTY error: {0}")]
    Pty(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Channel closed")]
    ChannelClosed,
}

/// Internal events for the shell runner event loop.
enum Event {
    Stdin(Vec<u8>),
    PtyOutput(Vec<u8>),
    PtyExit,
    Command(ShellCommand),
}

/// State for the status line overlay
struct StatusLine {
    /// Current status message
    message: Option<String>,
    /// Files that changed (shown during reload)
    changed_files: Vec<PathBuf>,
    /// Whether a reload is in progress
    reloading: bool,
}

impl StatusLine {
    fn new() -> Self {
        Self {
            message: None,
            changed_files: Vec::new(),
            reloading: false,
        }
    }
}

/// Raw terminal mode guard that restores terminal state on drop.
struct RawModeGuard {
    #[cfg(unix)]
    original: Option<libc::termios>,
}

impl RawModeGuard {
    fn new() -> io::Result<Self> {
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

/// PTY wrapper for easier management.
struct Pty {
    master: Box<dyn portable_pty::MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
}

impl Pty {
    fn spawn(cmd: CommandBuilder, size: PtySize) -> Result<Self, ShellRunnerError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(size)
            .map_err(|e| ShellRunnerError::Pty(e.to_string()))?;

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| ShellRunnerError::Pty(e.to_string()))?;

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| ShellRunnerError::Pty(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| ShellRunnerError::Pty(e.to_string()))?;

        Ok(Self {
            master: pair.master,
            child,
            reader,
            writer,
        })
    }

    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }

    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        self.writer.write_all(data)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }

    fn resize(&self, size: PtySize) -> Result<(), ShellRunnerError> {
        self.master
            .resize(size)
            .map_err(|e| ShellRunnerError::Pty(e.to_string()))
    }

    #[allow(dead_code)]
    fn try_wait(&mut self) -> Result<Option<portable_pty::ExitStatus>, ShellRunnerError> {
        self.child
            .try_wait()
            .map_err(|e| ShellRunnerError::Pty(e.to_string()))
    }

    fn kill(&mut self) -> Result<(), ShellRunnerError> {
        self.child
            .kill()
            .map_err(|e| ShellRunnerError::Pty(e.to_string()))
    }
}

/// Get current terminal size.
fn get_terminal_size() -> PtySize {
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

/// Shell runner that manages PTY and terminal I/O.
pub struct ShellRunner {
    /// Terminal size
    size: PtySize,
    /// Status line state
    status: StatusLine,
}

impl ShellRunner {
    /// Create a new shell runner.
    pub fn new() -> Self {
        let size = get_terminal_size();
        Self {
            size,
            status: StatusLine::new(),
        }
    }

    /// Run the shell with the given command channels.
    ///
    /// This function takes over the terminal and runs until the shell exits
    /// or the coordinator sends a shutdown command.
    ///
    /// Terminal handoff parameters (for TUI integration):
    /// - `backend_done_tx`: Sent after initial build to signal TUI to exit
    /// - `terminal_ready_rx`: Awaited before entering raw mode (TUI cleanup complete)
    pub async fn run(
        mut self,
        mut command_rx: mpsc::Receiver<ShellCommand>,
        event_tx: mpsc::Sender<ShellEvent>,
        backend_done_tx: tokio::sync::oneshot::Sender<()>,
        terminal_ready_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    ) -> Result<(), ShellRunnerError> {
        // Wait for the initial Spawn command
        let initial_cmd = match command_rx.recv().await {
            Some(ShellCommand::Spawn {
                command,
                watch_files,
            }) => {
                self.status.message = Some(format!("Watching {} files", watch_files.len()));
                command
            }
            Some(ShellCommand::Shutdown) | None => {
                return Ok(());
            }
            Some(other) => {
                return Err(ShellRunnerError::Pty(format!(
                    "Expected Spawn command, got {:?}",
                    other
                )));
            }
        };

        // Signal TUI that initial build is complete and we're ready for terminal
        let _ = backend_done_tx.send(());

        // Wait for TUI to release terminal (if running with TUI)
        if let Some(rx) = terminal_ready_rx {
            let _ = rx.await;
        }

        // Enter raw mode
        let _raw_guard = RawModeGuard::new()?;

        // Note: We intentionally don't use alternate screen to preserve shell history

        // Spawn initial PTY
        let pty = Arc::new(Mutex::new(Pty::spawn(initial_cmd, self.size)?));

        // Set up terminal state tracking
        let mut vt = Vt::new(self.size.cols as usize, self.size.rows as usize);

        // Set up event channel
        let (event_tx_internal, mut event_rx_internal) = mpsc::channel::<Event>(100);

        // Spawn stdin reader thread
        let stdin_tx = event_tx_internal.clone();
        std::thread::spawn(move || {
            let mut stdin = io::stdin();
            let mut buf = [0u8; 1024];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if stdin_tx
                            .blocking_send(Event::Stdin(buf[..n].to_vec()))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // Spawn PTY reader thread
        let pty_tx = event_tx_internal.clone();
        let pty_reader = pty.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                let result = {
                    let mut pty = pty_reader.lock().unwrap();
                    pty.read(&mut buf)
                };
                match result {
                    Ok(0) => {
                        let _ = pty_tx.blocking_send(Event::PtyExit);
                        break;
                    }
                    Ok(n) => {
                        if pty_tx
                            .blocking_send(Event::PtyOutput(buf[..n].to_vec()))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = pty_tx.blocking_send(Event::PtyExit);
                        break;
                    }
                }
            }
        });

        // Forward coordinator commands to internal event channel
        let cmd_tx = event_tx_internal.clone();
        tokio::spawn(async move {
            while let Some(cmd) = command_rx.recv().await {
                if cmd_tx.send(Event::Command(cmd)).await.is_err() {
                    break;
                }
            }
        });

        // Main event loop
        let result = self
            .event_loop(&pty, &mut vt, &mut event_rx_internal, &event_tx)
            .await;

        // Clean up
        {
            let mut pty_guard = pty.lock().unwrap();
            let _ = pty_guard.kill();
        }

        // Notify coordinator that shell exited
        let _ = event_tx.send(ShellEvent::Exited).await;

        result
    }

    /// Main event loop handling stdin, PTY output, and coordinator commands.
    async fn event_loop(
        &mut self,
        pty: &Arc<Mutex<Pty>>,
        vt: &mut Vt,
        event_rx: &mut mpsc::Receiver<Event>,
        coordinator_tx: &mpsc::Sender<ShellEvent>,
    ) -> Result<(), ShellRunnerError> {
        let mut stdout = io::stdout();

        while let Some(event) = event_rx.recv().await {
            match event {
                Event::Stdin(data) => {
                    // Write to PTY
                    let mut pty_guard = pty.lock().unwrap();
                    pty_guard.write_all(&data)?;
                    pty_guard.flush()?;
                }

                Event::PtyOutput(data) => {
                    // Feed to VT for state tracking
                    vt.feed_str(&String::from_utf8_lossy(&data));

                    // Write to stdout
                    stdout.write_all(&data)?;
                    stdout.flush()?;
                }

                Event::PtyExit => {
                    return Ok(());
                }

                Event::Command(cmd) => {
                    match cmd {
                        ShellCommand::Reload {
                            command,
                            changed_files,
                        } => {
                            // Note: VT state capture/replay is disabled because the dump()
                            // produces escape sequences that don't replay cleanly to a fresh terminal
                            let _state = vt.dump();

                            // Show reloading status
                            self.status.reloading = true;
                            self.status.changed_files = changed_files.clone();
                            self.draw_status_line(&mut stdout)?;

                            // Get new terminal size
                            let new_size = get_terminal_size();
                            self.size = new_size;

                            // Hold the lock for the entire swap operation to prevent
                            // the reader thread from seeing a killed PTY
                            let reload_result = {
                                let mut pty_guard = pty.lock().unwrap();
                                let _ = pty_guard.kill();

                                match Pty::spawn(command, new_size) {
                                    Ok(new_pty) => {
                                        *pty_guard = new_pty;

                                        // VT state replay disabled - the dump() escape sequences
                                        // don't work well with fresh terminals
                                        // let _ = stdout.write_all(_state.as_bytes());
                                        // let _ = stdout.flush();
                                        Ok(())
                                    }
                                    Err(e) => Err(e),
                                }
                            };

                            match reload_result {
                                Ok(()) => {
                                    // Reset VT
                                    *vt = Vt::new(new_size.cols as usize, new_size.rows as usize);

                                    // Update status
                                    self.status.reloading = false;
                                    let files_str: Vec<_> = changed_files
                                        .iter()
                                        .map(|p| p.display().to_string())
                                        .collect();
                                    self.status.message =
                                        Some(format!("Reloaded: {}", files_str.join(", ")));
                                    self.draw_status_line(&mut stdout)?;
                                }
                                Err(e) => {
                                    // Failed to spawn, show error
                                    let files_str: Vec<_> = changed_files
                                        .iter()
                                        .map(|p| p.display().to_string())
                                        .collect();
                                    self.status.message = Some(format!(
                                        "Reload failed ({}): {}",
                                        files_str.join(", "),
                                        e
                                    ));
                                    self.status.reloading = false;
                                    self.draw_status_line(&mut stdout)?;
                                }
                            }
                        }

                        ShellCommand::BuildFailed {
                            changed_files,
                            error,
                        } => {
                            // Show error in status line
                            let files_str: Vec<_> = changed_files
                                .iter()
                                .map(|p| p.display().to_string())
                                .collect();
                            self.status.message = Some(format!(
                                "Build failed ({}): {}",
                                files_str.join(", "),
                                error
                            ));
                            self.status.reloading = false;
                            self.draw_status_line(&mut stdout)?;
                        }

                        ShellCommand::Shutdown => {
                            return Ok(());
                        }

                        ShellCommand::Spawn { .. } => {
                            // Shouldn't receive Spawn after initial
                        }
                    }
                }
            }

            // Check for terminal resize
            let new_size = get_terminal_size();
            if new_size.cols != self.size.cols || new_size.rows != self.size.rows {
                self.size = new_size;
                let pty_guard = pty.lock().unwrap();
                let _ = pty_guard.resize(self.size);
                let _ = coordinator_tx
                    .send(ShellEvent::Resize {
                        cols: self.size.cols,
                        rows: self.size.rows,
                    })
                    .await;
            }
        }

        Ok(())
    }

    /// Draw the status line at the bottom of the terminal.
    fn draw_status_line(&self, stdout: &mut io::Stdout) -> Result<(), ShellRunnerError> {
        // Save cursor position
        execute!(stdout, cursor::SavePosition)?;

        // Move to the last line
        let status_row = self.size.rows.saturating_sub(1);
        execute!(stdout, cursor::MoveTo(0, status_row))?;

        // Clear the line
        execute!(stdout, terminal::Clear(ClearType::CurrentLine))?;

        // Build status text
        let status_text = if self.status.reloading {
            let files: Vec<_> = self
                .status
                .changed_files
                .iter()
                .map(|p| {
                    p.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                })
                .take(3)
                .collect();
            format!(" Reloading... [{}]", files.join(", "))
        } else if let Some(ref msg) = self.status.message {
            format!(" {}", msg)
        } else {
            " devenv shell (--reload)".to_string()
        };

        // Draw status with inverse colors
        let bg_color = if self.status.reloading {
            Color::Yellow
        } else {
            Color::DarkGrey
        };

        execute!(
            stdout,
            SetBackgroundColor(bg_color),
            SetForegroundColor(Color::White),
            Print(format!(
                "{:<width$}",
                status_text,
                width = self.size.cols as usize
            )),
            SetBackgroundColor(Color::Reset),
            SetForegroundColor(Color::Reset)
        )?;

        // Restore cursor position
        execute!(stdout, cursor::RestorePosition)?;
        stdout.flush()?;

        Ok(())
    }
}

impl Default for ShellRunner {
    fn default() -> Self {
        Self::new()
    }
}
