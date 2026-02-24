//! Shell session management.
//!
//! This module provides the main `ShellSession` type that orchestrates
//! PTY lifecycle, terminal I/O, status line, and task execution.

use crate::dec_mode::{DecModeEvent, DecModeScanner};
use crate::protocol::{PtyTaskRequest, ShellCommand, ShellEvent};
use crate::pty::{Pty, PtyError, get_terminal_size};
use crate::status_line::{SPINNER_INTERVAL_MS, StatusLine};
use crate::task_runner::PtyTaskRunner;
use crate::terminal::RawModeGuard;
use avt::Vt;
use crossterm::terminal;
use portable_pty::PtySize;
use std::fmt::Write as FmtWrite;
use std::io::{self, IsTerminal, Read, Write};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

/// Render a VT line as a string with SGR escape sequences.
///
/// Equivalent to the `Line::dump()` method that was public in avt 0.14
/// but made `pub(crate)` in 0.17.
fn dump_line(line: &avt::Line) -> String {
    let mut s = String::new();
    for cells in line.chunks(|c1, c2| c1.pen() != c2.pen()) {
        dump_pen(&mut s, cells[0].pen());
        for cell in &cells {
            s.push(cell.char());
        }
    }
    s
}

fn dump_pen(s: &mut String, pen: &avt::Pen) {
    s.push_str("\x1b[0");
    if let Some(c) = pen.foreground() {
        s.push(';');
        dump_color(s, c, 30);
    }
    if let Some(c) = pen.background() {
        s.push(';');
        dump_color(s, c, 40);
    }
    if pen.is_bold() {
        s.push_str(";1");
    }
    if pen.is_faint() {
        s.push_str(";2");
    }
    if pen.is_italic() {
        s.push_str(";3");
    }
    if pen.is_underline() {
        s.push_str(";4");
    }
    if pen.is_blink() {
        s.push_str(";5");
    }
    if pen.is_inverse() {
        s.push_str(";7");
    }
    if pen.is_strikethrough() {
        s.push_str(";9");
    }
    s.push('m');
}

fn dump_color(s: &mut String, color: avt::Color, base: u8) {
    match color {
        avt::Color::Indexed(c) if c < 8 => {
            let _ = write!(s, "{}", base + c);
        }
        avt::Color::Indexed(c) if c < 16 => {
            let _ = write!(s, "{}", base + 52 + c);
        }
        avt::Color::Indexed(c) => {
            let _ = write!(s, "{}:5:{}", base + 8, c);
        }
        avt::Color::RGB(rgb) => {
            let _ = write!(s, "{}:2:{}:{}:{}", base + 8, rgb.r, rgb.g, rgb.b);
        }
    }
}

/// Differential renderer that draws VT state to a bounded terminal region.
///
/// Instead of passing raw PTY output to stdout (which conflicts with the status
/// line's scroll region), this renderer mediates all terminal output through
/// the VT state machine — similar to how tmux works.
struct Renderer {
    /// Previous frame for diffing — one line of cells per row.
    prev_lines: Vec<Vec<avt::Cell>>,
    /// Previous cursor position and visibility.
    prev_cursor: (usize, usize, bool),
}

impl Renderer {
    fn new() -> Self {
        Self {
            prev_lines: Vec::new(),
            prev_cursor: (0, 0, true),
        }
    }

    /// Render changed VT lines to stdout. Skips lines that haven't changed.
    fn render(&mut self, stdout: &mut impl Write, vt: &Vt) -> io::Result<()> {
        for (row_idx, line) in vt.view().enumerate() {
            let cells = line.cells();
            if row_idx < self.prev_lines.len() && cells == &self.prev_lines[row_idx][..] {
                continue;
            }
            write!(stdout, "\x1b[{};1H\x1b[2K", row_idx + 1)?;
            stdout.write_all(dump_line(line).as_bytes())?;
            write!(stdout, "\x1b[0m")?;
            if row_idx < self.prev_lines.len() {
                self.prev_lines[row_idx] = cells.to_vec();
            } else {
                while self.prev_lines.len() < row_idx {
                    self.prev_lines.push(Vec::new());
                }
                self.prev_lines.push(cells.to_vec());
            }
        }
        self.update_cursor(stdout, vt)
    }

    /// Scroll the real terminal to push content into native scrollback, then render.
    ///
    /// When the VT reports scrolled lines (via `Changes.scrollback`), we briefly set
    /// a DECSTBM scroll region to protect the status line row, write newlines to scroll
    /// the real terminal, then reset the region. This pushes content into the terminal's
    /// native scrollback buffer — the same mechanism tmux uses.
    fn render_with_scroll(
        &mut self,
        stdout: &mut impl Write,
        vt: &Vt,
        scroll_count: usize,
        content_rows: u16,
    ) -> io::Result<()> {
        if scroll_count > 0 && content_rows > 0 {
            let effective = scroll_count.min(content_rows as usize);
            // Set scroll region to content area (protects status line row below)
            write!(stdout, "\x1b[1;{}r", content_rows)?;
            write!(stdout, "\x1b[{};1H", content_rows)?;
            let newlines = "\n".repeat(effective);
            stdout.write_all(newlines.as_bytes())?;
            // Reset scroll region to full terminal
            write!(stdout, "\x1b[r")?;
            // Shift prev_lines to match the scroll
            if effective < self.prev_lines.len() {
                self.prev_lines.drain(..effective);
            } else {
                self.prev_lines.clear();
            }
        }
        self.render(stdout, vt)
    }

    /// Full redraw of all VT lines (after resize or initialization).
    fn render_full(&mut self, stdout: &mut impl Write, vt: &Vt) -> io::Result<()> {
        self.prev_lines.clear();
        for (row_idx, line) in vt.view().enumerate() {
            write!(stdout, "\x1b[{};1H\x1b[2K", row_idx + 1)?;
            stdout.write_all(dump_line(line).as_bytes())?;
            write!(stdout, "\x1b[0m")?;
            self.prev_lines.push(line.cells().to_vec());
        }
        self.update_cursor(stdout, vt)
    }

    /// Position the real terminal cursor to match the VT cursor.
    fn update_cursor(&mut self, stdout: &mut impl Write, vt: &Vt) -> io::Result<()> {
        let cursor = vt.cursor();
        let new_cursor = (cursor.col, cursor.row, cursor.visible);
        if new_cursor != self.prev_cursor {
            if cursor.visible && !self.prev_cursor.2 {
                write!(stdout, "\x1b[?25h")?;
            } else if !cursor.visible && self.prev_cursor.2 {
                write!(stdout, "\x1b[?25l")?;
            }
            write!(stdout, "\x1b[{};{}H", cursor.row + 1, cursor.col + 1)?;
            self.prev_cursor = new_cursor;
        }
        Ok(())
    }

    /// Mark all lines as stale so the next render redraws everything.
    fn invalidate(&mut self) {
        self.prev_lines.clear();
    }
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
    /// Wait for TUI to release terminal. Receives the TUI's final render height.
    pub terminal_ready_rx: oneshot::Receiver<u16>,
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

/// Injectable I/O for the shell session.
/// When fields are None, real stdin/stdout are used.
#[derive(Default)]
pub struct SessionIo {
    pub stdin: Option<Box<dyn std::io::Read + Send>>,
    pub stdout: Option<Box<dyn std::io::Write + Send>>,
}

/// Internal events for the shell session event loop.
enum Event {
    Stdin(Vec<u8>),
    PtyOutput(Vec<u8>),
    PtyExit(Option<u32>),
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
        io: SessionIo,
    ) -> Result<Option<u32>, SessionError> {
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
                return Ok(None);
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
        let mut vt = Vt::builder()
            .size(pty_size.cols as usize, pty_size.rows as usize)
            .scrollback_limit(0)
            .build();

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

        let injected_stdin = io.stdin.is_some();
        let mut stdout: Box<dyn Write + Send> = io.stdout.unwrap_or_else(|| Box::new(io::stdout()));
        let stdin_source: Box<dyn Read + Send> = io.stdin.unwrap_or_else(|| Box::new(io::stdin()));

        // Query cursor position FIRST before any terminal resets.
        // This tells us where TUI left the cursor after its final render.
        // Skip when stdin is injected (not a real terminal) — the response comes
        // via stdin, so this would hang if stdin is not a TTY.
        let cursor_row = if !injected_stdin && io::stdin().is_terminal() {
            write!(stdout, "\x1b[6n")?;
            stdout.flush()?;

            let mut response = Vec::new();
            let mut stdin_real = io::stdin();
            let mut buf = [0u8; 1];
            loop {
                match stdin_real.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(_) if buf[0] != 0 => {
                        response.push(buf[0]);
                        if buf[0] == b'R' {
                            break;
                        }
                    }
                    Err(_) => break,
                    _ => {}
                }
                if response.len() > 20 {
                    break;
                }
            }

            // Parse cursor row from response: ESC [ row ; col R
            response
                .iter()
                .position(|&b| b == b'[')
                .and_then(|start| {
                    let s = String::from_utf8_lossy(
                        &response[start + 1..response.len().saturating_sub(1)],
                    );
                    s.split(';').next()?.parse::<u16>().ok()
                })
                .unwrap_or(1)
        } else {
            1
        };
        tracing::debug!("session: cursor position after TUI: row {}", cursor_row);

        // Get terminal size.
        // TODO: query the size from the actual stdout fd (e.g. TIOCGWINSZ on the
        // writer) instead of crossterm::terminal::size() which always uses the
        // process's controlling terminal. That would make this work correctly even
        // with injected I/O and remove the need for the config.size guard.
        if self.config.size.is_none()
            && let Ok((cols, rows)) = terminal::size()
        {
            self.size = PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            };
        }
        tracing::debug!(
            "session: terminal size: {}x{}",
            self.size.cols,
            self.size.rows
        );
        // Resize PTY to match current terminal size (minus status line row)
        let _ = pty.resize(self.pty_size());

        // Initialize the renderer and do a full initial draw
        let mut renderer = Renderer::new();
        renderer.render_full(&mut stdout, &vt)?;
        if self.config.show_status_line {
            self.status_line
                .draw(&mut stdout, self.size.cols, self.size.rows)?;
        }
        // Position cursor after initial draw
        let c = vt.cursor();
        write!(stdout, "\x1b[{};{}H", c.row + 1, c.col + 1)?;
        stdout.flush()?;

        // Set up event channel
        let (event_tx_internal, mut event_rx_internal) = mpsc::channel::<Event>(100);

        // Spawn stdin reader thread
        let stdin_tx = event_tx_internal.clone();
        std::thread::spawn(move || {
            let mut stdin = stdin_source;
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
                        let exit_code = pty_reader.try_wait().ok().flatten().map(|s| s.exit_code());
                        let _ = pty_tx.blocking_send(Event::PtyExit(exit_code));
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
                        let exit_code = pty_reader.try_wait().ok().flatten().map(|s| s.exit_code());
                        let _ = pty_tx.blocking_send(Event::PtyExit(exit_code));
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
        let exit_code = self
            .event_loop(
                &pty,
                &mut vt,
                &mut renderer,
                &mut event_rx_internal,
                &event_tx,
                &mut stdout,
            )
            .await;

        let _ = pty.kill();

        let exit_code = exit_code?;

        // Notify coordinator that shell exited
        let _ = event_tx.send(ShellEvent::Exited { exit_code }).await;

        Ok(exit_code)
    }

    /// Main event loop handling stdin, PTY output, and coordinator commands.
    /// Returns the exit code from the PTY child process, if available.
    async fn event_loop(
        &mut self,
        pty: &Arc<Pty>,
        vt: &mut Vt,
        renderer: &mut Renderer,
        event_rx: &mut mpsc::Receiver<Event>,
        coordinator_tx: &mpsc::Sender<ShellEvent>,
        stdout: &mut Box<dyn Write + Send>,
    ) -> Result<Option<u32>, SessionError> {
        let spinner_interval = Duration::from_millis(SPINNER_INTERVAL_MS);
        let mut last_resize_check = std::time::Instant::now();
        let mut scanner = DecModeScanner::new();
        let mut in_alternate_screen = false;
        // Track forwarded modes so we can clean them up on exit
        let mut forwarded_mouse_modes: Vec<u16> = Vec::new();

        loop {
            // Use select! to handle both events and spinner animation
            let event = if self.status_line.state().building {
                tokio::select! {
                    event = event_rx.recv() => event,
                    _ = tokio::time::sleep(spinner_interval) => {
                        if self.config.show_status_line {
                            self.status_line.draw(stdout, self.size.cols, self.size.rows)?;
                            let c = vt.cursor();
                            write!(stdout, "\x1b[{};{}H", c.row + 1, c.col + 1)?;
                            stdout.flush()?;
                        }
                        continue;
                    }
                }
            } else {
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
                    // Check for Ctrl-Alt-E (ESC + Ctrl-E = 0x1b 0x05) to toggle error
                    if data.len() == 2 && data[0] == 0x1b && data[1] == 0x05 {
                        let state = self.status_line.state_mut();
                        if state.error.is_some() {
                            state.show_error = !state.show_error;
                            if state.show_error {
                                let error = state.error.clone().unwrap();
                                let mut error_text =
                                    String::from("\r\n\x1b[1;31mBuild error:\x1b[0m\r\n");
                                for line in error.lines() {
                                    error_text.push_str(&format!("  {}\r\n", line));
                                }
                                error_text.push_str("\r\n");
                                let scroll_count = {
                                    let changes = vt.feed_str(&error_text);
                                    changes.scrollback.count()
                                };
                                let content_rows = self.pty_size().rows;
                                renderer.render_with_scroll(
                                    stdout,
                                    vt,
                                    scroll_count,
                                    content_rows,
                                )?;
                            } else {
                                pty.write_all(&[0x0C])?;
                                pty.flush()?;
                            }
                            self.status_line
                                .draw(stdout, self.size.cols, self.size.rows)?;
                            let c = vt.cursor();
                            write!(stdout, "\x1b[{};{}H", c.row + 1, c.col + 1)?;
                            stdout.flush()?;
                        }
                        continue;
                    }
                    pty.write_all(&data)?;
                    pty.flush()?;
                }

                Event::PtyOutput(data) => {
                    // Scan for DEC private mode sequences and forward them
                    let was_in_alt = in_alternate_screen;
                    Self::process_dec_events(
                        &mut scanner,
                        &data,
                        &mut in_alternate_screen,
                        &mut forwarded_mouse_modes,
                        stdout,
                    )?;

                    // Feed output into VT and track how many lines scrolled off
                    let mut total_scroll: usize = 0;
                    {
                        let changes = vt.feed_str(&String::from_utf8_lossy(&data));
                        total_scroll += changes.scrollback.count();
                    }

                    // Batch: drain any additional pending PtyOutput events
                    while let Ok(event) = event_rx.try_recv() {
                        match event {
                            Event::PtyOutput(more) => {
                                Self::process_dec_events(
                                    &mut scanner,
                                    &more,
                                    &mut in_alternate_screen,
                                    &mut forwarded_mouse_modes,
                                    stdout,
                                )?;
                                let changes = vt.feed_str(&String::from_utf8_lossy(&more));
                                total_scroll += changes.scrollback.count();
                            }
                            Event::PtyExit(exit_code) => {
                                Self::cleanup_forwarded_modes(
                                    in_alternate_screen,
                                    &forwarded_mouse_modes,
                                    stdout,
                                )?;
                                renderer.render(stdout, vt)?;
                                return Ok(exit_code);
                            }
                            Event::Stdin(stdin_data) => {
                                pty.write_all(&stdin_data)?;
                                pty.flush()?;
                            }
                            Event::Command(cmd) => {
                                self.handle_command(cmd, stdout, vt, renderer)?;
                            }
                        }
                    }

                    // Handle alternate screen transitions
                    if was_in_alt != in_alternate_screen {
                        renderer.invalidate();
                    }

                    if in_alternate_screen {
                        renderer.render(stdout, vt)?;
                    } else {
                        let content_rows = self.pty_size().rows;
                        renderer.render_with_scroll(stdout, vt, total_scroll, content_rows)?;
                    }
                    if self.config.show_status_line {
                        self.status_line
                            .draw(stdout, self.size.cols, self.size.rows)?;
                    }
                    let c = vt.cursor();
                    write!(stdout, "\x1b[{};{}H", c.row + 1, c.col + 1)?;
                    stdout.flush()?;
                }

                Event::PtyExit(exit_code) => {
                    Self::cleanup_forwarded_modes(
                        in_alternate_screen,
                        &forwarded_mouse_modes,
                        stdout,
                    )?;
                    return Ok(exit_code);
                }

                Event::Command(cmd) => {
                    self.handle_command(cmd, stdout, vt, renderer)?;
                }
            }

            // Check for terminal resize periodically
            if last_resize_check.elapsed() > Duration::from_millis(500) {
                last_resize_check = std::time::Instant::now();
                if let Ok((cols, rows)) = terminal::size()
                    && (cols != self.size.cols || rows != self.size.rows)
                {
                    self.size = PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    };
                    let pty_size = self.pty_size();
                    let _ = pty.resize(pty_size);
                    vt.resize(pty_size.cols as usize, pty_size.rows as usize);
                    renderer.invalidate();
                    renderer.render_full(stdout, vt)?;
                    if self.config.show_status_line && !in_alternate_screen {
                        self.status_line.draw(stdout, cols, rows)?;
                    }
                    let c = vt.cursor();
                    write!(stdout, "\x1b[{};{}H", c.row + 1, c.col + 1)?;
                    stdout.flush()?;
                    let _ = coordinator_tx
                        .send(ShellEvent::Resize {
                            cols: self.size.cols,
                            rows: self.size.rows,
                        })
                        .await;
                }
            }
        }

        Self::cleanup_forwarded_modes(in_alternate_screen, &forwarded_mouse_modes, stdout)?;
        Ok(None)
    }

    /// Handle a command from the coordinator.
    fn handle_command(
        &mut self,
        cmd: ShellCommand,
        stdout: &mut Box<dyn Write + Send>,
        vt: &mut Vt,
        renderer: &mut Renderer,
    ) -> Result<(), SessionError> {
        match cmd {
            ShellCommand::ReloadReady { changed_files } => {
                self.status_line.state_mut().set_reload_ready(changed_files);
                self.draw_status_and_cursor(stdout, vt)?;
            }

            ShellCommand::Building { changed_files } => {
                self.status_line.state_mut().set_building(changed_files);
                self.draw_status_and_cursor(stdout, vt)?;
            }

            ShellCommand::BuildFailed {
                changed_files,
                error,
            } => {
                self.status_line
                    .state_mut()
                    .set_build_failed(changed_files, error);
                self.draw_status_and_cursor(stdout, vt)?;
            }

            ShellCommand::ReloadApplied => {
                self.status_line.state_mut().clear();
                self.draw_status_and_cursor(stdout, vt)?;
            }

            ShellCommand::WatchedFiles { files } => {
                self.status_line.state_mut().set_watched_files(files);
                self.draw_status_and_cursor(stdout, vt)?;
            }

            ShellCommand::WatchingPaused { paused } => {
                self.status_line.state_mut().set_paused(paused);
                self.draw_status_and_cursor(stdout, vt)?;
            }

            ShellCommand::PrintWatchedFiles { files } => {
                // Inject watched files list into VT so it's tracked
                let mut text = format!("\r\n\x1b[1mWatched files ({}):\x1b[0m\r\n", files.len());
                for file in &files {
                    text.push_str(&format!("  {}\r\n", file.display()));
                }
                let scroll_count = {
                    let changes = vt.feed_str(&text);
                    changes.scrollback.count()
                };
                let content_rows = self.pty_size().rows;
                renderer.render_with_scroll(stdout, vt, scroll_count, content_rows)?;
                self.draw_status_and_cursor(stdout, vt)?;
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

    /// Scan raw PTY output for DEC private mode sequences, forward relevant
    /// ones to the real terminal, and update alternate screen state.
    fn process_dec_events(
        scanner: &mut DecModeScanner,
        data: &[u8],
        in_alternate_screen: &mut bool,
        forwarded_mouse_modes: &mut Vec<u16>,
        stdout: &mut impl Write,
    ) -> io::Result<()> {
        for event in scanner.scan(data) {
            if !event.has_forwarded_mode() {
                continue;
            }
            stdout.write_all(event.raw_bytes())?;

            if event.enters_alt_screen() {
                *in_alternate_screen = true;
            } else if event.exits_alt_screen() {
                *in_alternate_screen = false;
            }

            // Track mouse modes for cleanup on exit
            match &event {
                DecModeEvent::Set { modes, .. } => {
                    for &m in modes {
                        if matches!(m, 1000 | 1002 | 1003 | 1005 | 1006 | 1015 | 2004 | 1004)
                            && !forwarded_mouse_modes.contains(&m)
                        {
                            forwarded_mouse_modes.push(m);
                        }
                    }
                }
                DecModeEvent::Reset { modes, .. } => {
                    forwarded_mouse_modes.retain(|m| !modes.contains(m));
                }
            }
        }
        Ok(())
    }

    /// Reset any forwarded DEC modes on exit so the terminal is left clean.
    fn cleanup_forwarded_modes(
        in_alternate_screen: bool,
        forwarded_mouse_modes: &[u16],
        stdout: &mut impl Write,
    ) -> io::Result<()> {
        if in_alternate_screen {
            stdout.write_all(b"\x1b[?1049l")?;
        }
        for &mode in forwarded_mouse_modes {
            write!(stdout, "\x1b[?{}l", mode)?;
        }
        if in_alternate_screen || !forwarded_mouse_modes.is_empty() {
            stdout.flush()?;
        }
        Ok(())
    }

    /// Draw status line and reposition cursor.
    fn draw_status_and_cursor(
        &mut self,
        stdout: &mut Box<dyn Write + Send>,
        vt: &Vt,
    ) -> Result<(), SessionError> {
        if self.config.show_status_line {
            self.status_line
                .draw(stdout, self.size.cols, self.size.rows)?;
            let c = vt.cursor();
            write!(stdout, "\x1b[{};{}H", c.row + 1, c.col + 1)?;
            stdout.flush()?;
        }
        Ok(())
    }
}

impl Default for ShellSession {
    fn default() -> Self {
        Self::with_defaults()
    }
}
