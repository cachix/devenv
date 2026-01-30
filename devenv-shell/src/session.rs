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
use portable_pty::PtySize;
use std::io::{self, Read, Write};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

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
        let pty = Arc::new(Pty::spawn(initial_cmd, self.size)?);
        let mut vt = Vt::new(self.size.cols as usize, self.size.rows as usize);

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

        // Nudge the shell to render a fresh prompt after terminal handoff
        if self.config.show_status_line {
            pty.write_all(b"\n")?;
            pty.flush()?;
        }

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

        // Clean up
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

        loop {
            // Use select! to handle both events and spinner animation
            let event = if self.status_line.state().building {
                // When building, use a timeout to animate the spinner
                tokio::select! {
                    event = event_rx.recv() => event,
                    _ = tokio::time::sleep(spinner_interval) => {
                        // Redraw status line to animate spinner
                        self.status_line.draw(&mut stdout, self.size.rows, self.size.cols)?;
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
                    pty.write_all(&data)?;
                    pty.flush()?;
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
                    self.handle_command(cmd, &mut stdout)?;
                }
            }

            // Check for terminal resize
            let new_size = get_terminal_size();
            if new_size.cols != self.size.cols || new_size.rows != self.size.rows {
                self.size = new_size;
                let _ = pty.resize(self.size);
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

    /// Handle a command from the coordinator.
    fn handle_command(
        &mut self,
        cmd: ShellCommand,
        stdout: &mut io::Stdout,
    ) -> Result<(), SessionError> {
        match cmd {
            ShellCommand::ReloadReady { changed_files } => {
                let keybind = std::env::var("DEVENV_RELOAD_KEYBIND")
                    .unwrap_or_else(|_| "Alt-Ctrl-R".to_string());
                self.status_line
                    .state_mut()
                    .set_reload_ready(changed_files, &keybind);
                self.status_line
                    .draw(stdout, self.size.rows, self.size.cols)?;
            }

            ShellCommand::Building { changed_files } => {
                self.status_line.state_mut().set_building(changed_files);
                self.status_line
                    .draw(stdout, self.size.rows, self.size.cols)?;
            }

            ShellCommand::BuildFailed {
                changed_files,
                error,
            } => {
                self.status_line
                    .state_mut()
                    .set_build_failed(changed_files, error);
                self.status_line
                    .draw(stdout, self.size.rows, self.size.cols)?;
            }

            ShellCommand::ReloadApplied => {
                self.status_line.state_mut().clear();
                self.status_line
                    .draw(stdout, self.size.rows, self.size.cols)?;
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
