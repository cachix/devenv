//! Shell session management.
//!
//! This module provides the main `ShellSession` type that orchestrates
//! PTY lifecycle, terminal I/O, status line, and task execution.

use crate::protocol::{PtyTaskRequest, ShellCommand, ShellEvent};
use crate::pty::{Pty, PtyError, get_terminal_size};
use crate::status_line::{SPINNER_INTERVAL_MS, StatusLine};
use crate::task_runner::PtyTaskRunner;
use crate::terminal::RawModeGuard;
use avt::Vt;
use crossterm::{cursor, execute, terminal};
use portable_pty::PtySize;
use std::io::{self, Read, Write};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

/// Set terminal scroll region (DECSTBM). Rows are 1-indexed.
/// This restricts scrolling to the specified region, leaving other rows fixed.
fn set_scroll_region(stdout: &mut impl Write, top: u16, bottom: u16) -> io::Result<()> {
    write!(stdout, "\x1b[{};{}r", top, bottom)?;
    stdout.flush()
}

/// Reset scroll region to full terminal.
fn reset_scroll_region(stdout: &mut impl Write) -> io::Result<()> {
    write!(stdout, "\x1b[r")?;
    stdout.flush()
}

/// Check if data contains terminal clear/reset sequences.
/// This detects: \x1b[2J (clear screen), \x1b[3J (clear scrollback), \x1bc (reset terminal)
fn contains_clear_sequence(data: &[u8]) -> bool {
    // Look for escape sequences that clear the terminal
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0x1b {
            // ESC character
            if i + 1 < data.len() {
                match data[i + 1] {
                    b'c' => return true, // ESC c = reset terminal
                    b'[' => {
                        // CSI sequence - look for 2J, 3J, or H followed by 2J
                        let mut j = i + 2;
                        while j < data.len() && (data[j].is_ascii_digit() || data[j] == b';') {
                            j += 1;
                        }
                        if j < data.len() {
                            // Check for clear screen sequences
                            if data[j] == b'J' && j > i + 2 {
                                let param = &data[i + 2..j];
                                if param == b"2" || param == b"3" {
                                    return true;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        i += 1;
    }
    false
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("PTY error: {0}")]
    Pty(#[from] PtyError),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("channel closed")]
    ChannelClosed,
    #[error("unexpected command: expected Spawn, got {0}")]
    UnexpectedCommand(String),
    #[error("task runner error: {0}")]
    TaskRunner(#[from] crate::task_runner::TaskRunnerError),
}

/// Configuration for TUI handoff.
///
/// When running with TUI, the shell session needs to coordinate
/// terminal ownership with the TUI.
pub struct TuiHandoff {
    /// Signal when backend work is done (TUI can exit).
    pub backend_done_tx: oneshot::Sender<()>,
    /// Wait for TUI to release terminal.
    pub terminal_ready_rx: oneshot::Receiver<()>,
    /// Optional channel to receive task execution requests.
    pub task_rx: Option<mpsc::Receiver<PtyTaskRequest>>,
    /// Optional channel to signal PTY is ready for tasks.
    pub pty_ready_tx: Option<oneshot::Sender<()>>,
}

/// Shell session configuration.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Show status line at bottom of terminal.
    pub show_status_line: bool,
    /// Initial terminal size (auto-detected if None).
    pub size: Option<PtySize>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            show_status_line: true,
            size: None,
        }
    }
}

/// Internal events for the shell session event loop.
enum Event {
    Stdin(Vec<u8>),
    PtyOutput(Vec<u8>),
    PtyExit,
    Command(ShellCommand),
}

/// Interactive shell session with hot-reload support.
///
/// Manages PTY lifecycle, terminal I/O, status line, and task execution.
pub struct ShellSession {
    config: SessionConfig,
    size: PtySize,
    status_line: StatusLine,
}

impl ShellSession {
    /// Create a new shell session with the given configuration.
    pub fn new(config: SessionConfig) -> Self {
        let size = config.size.unwrap_or_else(get_terminal_size);
        let mut status_line = StatusLine::with_defaults();
        status_line.set_enabled(config.show_status_line);

        Self {
            config,
            size,
            status_line,
        }
    }

    /// Get the PTY size, reserving 1 row for status line if enabled.
    fn pty_size(&self) -> PtySize {
        if self.config.show_status_line {
            PtySize {
                rows: self.size.rows.saturating_sub(1).max(1),
                cols: self.size.cols,
                ..self.size
            }
        } else {
            self.size
        }
    }

    /// Set up scroll region to reserve bottom row for status line.
    fn setup_scroll_region(&self, stdout: &mut impl Write) -> io::Result<()> {
        if self.config.show_status_line && self.size.rows > 1 {
            set_scroll_region(stdout, 1, self.size.rows - 1)
        } else {
            Ok(())
        }
    }

    /// Create a new shell session with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(SessionConfig::default())
    }

    /// Set whether to show the status line.
    pub fn with_status_line(mut self, show: bool) -> Self {
        self.config.show_status_line = show;
        self.status_line.set_enabled(show);
        self
    }

    /// Run the shell session.
    ///
    /// This function takes over the terminal and runs until the shell exits
    /// or the coordinator sends a shutdown command.
    ///
    /// # Arguments
    /// * `command_rx` - Receives commands from coordinator
    /// * `event_tx` - Sends events to coordinator
    /// * `handoff` - Optional TUI handoff configuration
    pub async fn run(
        mut self,
        mut command_rx: mpsc::Receiver<ShellCommand>,
        event_tx: mpsc::Sender<ShellEvent>,
        handoff: Option<TuiHandoff>,
    ) -> Result<(), SessionError> {
        // Wait for the initial Spawn command
        let (initial_cmd, _watch_files) = match command_rx.recv().await {
            Some(ShellCommand::Spawn {
                command,
                watch_files,
            }) => {
                self.status_line
                    .state_mut()
                    .set_message(format!("Watching {} files", watch_files.len()));
                (command, watch_files)
            }
            Some(ShellCommand::Shutdown) | None => {
                if let Some(h) = handoff {
                    let _ = h.backend_done_tx.send(());
                }
                return Ok(());
            }
            Some(other) => {
                if let Some(h) = handoff {
                    let _ = h.backend_done_tx.send(());
                }
                return Err(SessionError::UnexpectedCommand(format!("{:?}", other)));
            }
        };

        // Spawn PTY early so tasks can run in it (before TUI exits)
        // Reserve 1 row for status line if enabled
        let pty_size = self.pty_size();
        let pty = Arc::new(Pty::spawn(initial_cmd, pty_size)?);
        let mut vt = Vt::new(pty_size.cols as usize, pty_size.rows as usize);

        // Handle TUI handoff if present
        if let Some(mut handoff) = handoff {
            // Signal that PTY is ready for tasks
            if let Some(tx) = handoff.pty_ready_tx.take() {
                let _ = tx.send(());
            }

            // Run any tasks in the PTY (TUI still active, showing progress)
            if let Some(mut task_rx) = handoff.task_rx.take() {
                let task_runner = PtyTaskRunner::new(Arc::clone(&pty));
                task_runner.run_with_vt(&mut task_rx, &mut vt).await?;
            }

            // Signal TUI that initial build is complete and we're ready for terminal
            tracing::trace!("session: sending backend_done_tx");
            let _ = handoff.backend_done_tx.send(());

            // Wait for TUI to release terminal
            tracing::trace!("session: waiting for terminal_ready_rx");
            let _ = handoff.terminal_ready_rx.await;
            tracing::trace!("session: terminal_ready_rx received");
        }

        // Enter raw mode
        tracing::trace!("session: entering raw mode");
        let _raw_guard = RawModeGuard::new()?;
        tracing::trace!("session: raw mode active");

        let mut stdout = io::stdout();

        // Reset terminal state from TUI: scroll region and origin mode
        write!(stdout, "\x1b[r\x1b[?6l")?;
        stdout.flush()?;

        // Query actual terminal size by moving cursor to bottom-right and reading position
        // This is more reliable than terminal::size() which can return wrong values
        write!(stdout, "\x1b[999;999H\x1b[6n")?;
        stdout.flush()?;

        // Read cursor position response: ESC [ rows ; cols R
        // We need to read from stdin in raw mode
        let mut response = Vec::new();
        let mut stdin = io::stdin();
        let mut buf = [0u8; 1];
        loop {
            if stdin.read(&mut buf).is_ok() && buf[0] != 0 {
                response.push(buf[0]);
                if buf[0] == b'R' {
                    break;
                }
            }
            if response.len() > 20 {
                break; // Safety limit
            }
        }

        // Parse response: ESC [ rows ; cols R
        if let Some(pos) = response.iter().position(|&b| b == b'[').and_then(|start| {
            let s = String::from_utf8_lossy(&response[start + 1..response.len() - 1]);
            let parts: Vec<&str> = s.split(';').collect();
            if parts.len() == 2 {
                Some((
                    parts[0].parse::<u16>().unwrap_or(24),
                    parts[1].parse::<u16>().unwrap_or(80),
                ))
            } else {
                None
            }
        }) {
            self.size = PtySize {
                rows: pos.0,
                cols: pos.1,
                pixel_width: 0,
                pixel_height: 0,
            };
            tracing::debug!(
                "session: actual terminal size (via cursor query): {}x{}",
                self.size.cols,
                self.size.rows
            );
        } else {
            // Fallback to crossterm
            if let Ok((cols, rows)) = terminal::size() {
                self.size = PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                };
            }
            tracing::debug!(
                "session: terminal size (fallback): {}x{}",
                self.size.cols,
                self.size.rows
            );
        }
        // Resize PTY to match current terminal size (minus status line row)
        let _ = pty.resize(self.pty_size());

        // Set up scroll region to reserve bottom row for status line
        if self.config.show_status_line {
            self.setup_scroll_region(&mut stdout)?;
            // Move cursor to top-left and clear the scroll region
            execute!(stdout, cursor::MoveTo(0, 0))?;
            execute!(stdout, terminal::Clear(terminal::ClearType::FromCursorDown))?;
            // Draw status line at absolute bottom
            self.status_line.draw(&mut stdout, self.size.cols)?;
        }

        // Send Ctrl-L to shell to redraw prompt cleanly
        pty.write_all(b"\x0c")?;
        pty.flush()?;

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
                    Err(e) => {
                        tracing::warn!("session: stdin read error: {}", e);
                        break;
                    }
                }
            }
        });

        // Spawn PTY reader thread
        let pty_tx = event_tx_internal.clone();
        let pty_reader = Arc::clone(&pty);
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match pty_reader.read(&mut buf) {
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
                    Err(e) => {
                        tracing::warn!("session: PTY read error: {}", e);
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
        tracing::trace!("session: starting event loop");
        let result = self
            .event_loop(&pty, &mut vt, &mut event_rx_internal, &event_tx)
            .await;

        // Clean up - reset scroll region before exiting
        if self.config.show_status_line {
            let _ = reset_scroll_region(&mut stdout);
        }
        let _ = pty.kill();

        // Notify coordinator that shell exited
        let _ = event_tx.send(ShellEvent::Exited).await;

        result
    }

    /// Main event loop handling stdin, PTY output, and coordinator commands.
    async fn event_loop(
        &mut self,
        pty: &Arc<Pty>,
        vt: &mut Vt,
        event_rx: &mut mpsc::Receiver<Event>,
        coordinator_tx: &mpsc::Sender<ShellEvent>,
    ) -> Result<(), SessionError> {
        let mut stdout = io::stdout();
        let spinner_interval = Duration::from_millis(SPINNER_INTERVAL_MS);
        let mut last_resize_check = std::time::Instant::now();

        loop {
            // Use select! to handle both events and spinner animation
            let event = if self.status_line.state().building {
                // When building, use a timeout to animate the spinner
                tokio::select! {
                    event = event_rx.recv() => event,
                    _ = tokio::time::sleep(spinner_interval) => {
                        // Redraw status line to animate spinner
                        self.status_line.draw(&mut stdout, self.size.cols)?;
                        continue;
                    }
                }
            } else {
                // When not building, just wait for events
                event_rx.recv().await
            };

            let Some(event) = event else {
                break;
            };

            match event {
                Event::Stdin(data) => {
                    // Check for Ctrl-Alt-D (ESC + Ctrl-D = 0x1b 0x04) to toggle pause
                    if data.len() == 2 && data[0] == 0x1b && data[1] == 0x04 {
                        let _ = coordinator_tx.send(ShellEvent::TogglePause).await;
                        continue;
                    }
                    // Check for Ctrl-Alt-W (ESC + Ctrl-W = 0x1b 0x17) to list watched files
                    if data.len() == 2 && data[0] == 0x1b && data[1] == 0x17 {
                        let _ = coordinator_tx.send(ShellEvent::ListWatchedFiles).await;
                        continue;
                    }
                    pty.write_all(&data)?;
                    pty.flush()?;
                }

                Event::PtyOutput(data) => {
                    // Write to stdout immediately
                    stdout.write_all(&data)?;
                    stdout.flush()?;

                    // Feed to VT for state tracking (used during reload)
                    vt.feed_str(&String::from_utf8_lossy(&data));

                    // Check for terminal clear sequences and redraw status line
                    if self.config.show_status_line && contains_clear_sequence(&data) {
                        let _ = self.setup_scroll_region(&mut stdout);
                        let _ = self.status_line.draw(&mut stdout, self.size.cols);
                    }
                }

                Event::PtyExit => {
                    return Ok(());
                }

                Event::Command(cmd) => {
                    self.handle_command(cmd, &mut stdout)?;
                }
            }

            // Check for terminal resize periodically (not on every event)
            if last_resize_check.elapsed() > Duration::from_millis(500) {
                last_resize_check = std::time::Instant::now();
                if let Ok((cols, _rows)) = terminal::size() {
                    if cols != self.size.cols {
                        self.size.cols = cols;
                        let _ = coordinator_tx
                            .send(ShellEvent::Resize {
                                cols: self.size.cols,
                                rows: self.size.rows,
                            })
                            .await;
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle a command from the coordinator.
    fn handle_command(
        &mut self,
        cmd: ShellCommand,
        stdout: &mut io::Stdout,
    ) -> Result<(), SessionError> {
        match cmd {
            ShellCommand::ReloadReady { changed_files } => {
                self.status_line.state_mut().set_reload_ready(changed_files);
                self.status_line.draw(stdout, self.size.cols)?;
            }

            ShellCommand::Building { changed_files } => {
                self.status_line.state_mut().set_building(changed_files);
                self.status_line.draw(stdout, self.size.cols)?;
            }

            ShellCommand::BuildFailed {
                changed_files,
                error,
            } => {
                self.status_line
                    .state_mut()
                    .set_build_failed(changed_files, error);
                self.status_line.draw(stdout, self.size.cols)?;
            }

            ShellCommand::ReloadApplied => {
                self.status_line.state_mut().clear();
                self.status_line.draw(stdout, self.size.cols)?;
            }

            ShellCommand::WatchedFiles { files } => {
                self.status_line.state_mut().set_watched_files(files);
                self.status_line.draw(stdout, self.size.cols)?;
            }

            ShellCommand::WatchingPaused { paused } => {
                self.status_line.state_mut().set_paused(paused);
                self.status_line.draw(stdout, self.size.cols)?;
            }

            ShellCommand::PrintWatchedFiles { files } => {
                // Print watched files list
                writeln!(
                    stdout,
                    "\r\n\x1b[1mWatched files ({}):\x1b[0m\r",
                    files.len()
                )?;
                for file in &files {
                    writeln!(stdout, "  {}\r", file.display())?;
                }
                stdout.flush()?;
            }

            ShellCommand::Shutdown => {
                // Will be handled by returning from event loop
            }

            ShellCommand::Spawn { .. } => {
                // Shouldn't receive Spawn after initial
            }
        }

        Ok(())
    }
}

impl Default for ShellSession {
    fn default() -> Self {
        Self::with_defaults()
    }
}
