//! Shell session management.
//!
//! This module provides the main `ShellSession` type that orchestrates
//! PTY lifecycle, terminal I/O, status line, and task execution.

use crate::control_pipe::ControlPipe;
use crate::escape::{DecModeEvent, EscapeScanner, SequenceEvent};
use crate::protocol::{PtyTaskRequest, ShellCommand, ShellEvent};
use crate::pty::{Pty, PtyError, get_terminal_size};
use crate::status_line::{SPINNER_INTERVAL_MS, StatusLine};
use crate::task_runner::PtyTaskRunner;
use crate::terminal::RawModeGuard;
use crate::utf8_accumulator::Utf8Accumulator;
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

/// Feed text into VT and return the actual number of lines that scrolled off
/// the viewport. Unlike `changes.scrollback.count()` (which only reports lines
/// trimmed by gc beyond the scrollback limit), this accounts for lines retained
/// in scrollback too.
fn feed_vt(vt: &mut Vt, text: &str) -> usize {
    let lines_before = vt.lines().count();
    let gc_count = {
        let changes = vt.feed_str(text);
        changes.scrollback.count()
    };
    let lines_after = vt.lines().count();
    (lines_after + gc_count).saturating_sub(lines_before)
}

/// Filters OSC responses from stdin to prevent garbled text.
///
/// When we forward OSC queries (e.g., color scheme detection) to the real
/// terminal, the terminal's responses arrive on stdin. If the querying
/// program has exited before the response arrives, the response bytes
/// would be interpreted as user input by readline. This filter removes
/// OSC response sequences (`ESC ] <digits> ; <payload> <terminator>`)
/// from the stdin stream while passing everything else through.
struct StdinFilter {
    state: StdinFilterState,
    buf: Vec<u8>,
}

#[derive(Clone, Copy)]
enum StdinFilterState {
    Ground,
    Esc,
    OscDigit,
    OscPayload,
    OscPayloadEsc,
}

impl StdinFilter {
    fn new() -> Self {
        Self {
            state: StdinFilterState::Ground,
            buf: Vec::new(),
        }
    }

    /// Filter a chunk of stdin data, returning only non-OSC bytes.
    fn filter(&mut self, data: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(data.len());

        for &byte in data {
            match self.state {
                StdinFilterState::Ground => {
                    if byte == 0x1b {
                        self.buf.clear();
                        self.buf.push(byte);
                        self.state = StdinFilterState::Esc;
                    } else {
                        output.push(byte);
                    }
                }
                StdinFilterState::Esc => {
                    if byte == b']' {
                        self.buf.push(byte);
                        self.state = StdinFilterState::OscDigit;
                    } else if byte == 0x1b {
                        output.push(0x1b);
                        self.buf.clear();
                        self.buf.push(byte);
                    } else {
                        output.extend_from_slice(&self.buf);
                        output.push(byte);
                        self.buf.clear();
                        self.state = StdinFilterState::Ground;
                    }
                }
                StdinFilterState::OscDigit => {
                    if byte.is_ascii_digit() {
                        self.buf.push(byte);
                    } else if byte == b';' {
                        self.buf.push(byte);
                        self.state = StdinFilterState::OscPayload;
                    } else {
                        // Not a valid OSC response pattern, emit everything
                        output.extend_from_slice(&self.buf);
                        output.push(byte);
                        self.buf.clear();
                        self.state = StdinFilterState::Ground;
                    }
                }
                StdinFilterState::OscPayload => {
                    if byte == 0x07 {
                        // BEL terminates OSC — drop the entire sequence
                        self.buf.clear();
                        self.state = StdinFilterState::Ground;
                    } else if byte == 0x1b {
                        self.buf.push(byte);
                        self.state = StdinFilterState::OscPayloadEsc;
                    } else {
                        self.buf.push(byte);
                        if self.buf.len() > 256 {
                            // Safety limit: not a real OSC response, emit
                            output.extend_from_slice(&self.buf);
                            self.buf.clear();
                            self.state = StdinFilterState::Ground;
                        }
                    }
                }
                StdinFilterState::OscPayloadEsc => {
                    if byte == b'\\' {
                        // ST (ESC \) terminates OSC — drop the entire sequence
                        self.buf.clear();
                        self.state = StdinFilterState::Ground;
                    } else if byte == 0x1b {
                        // Another ESC — give up on this OSC, start fresh
                        output.extend_from_slice(&self.buf[..self.buf.len() - 1]);
                        self.buf.clear();
                        self.buf.push(byte);
                        self.state = StdinFilterState::Esc;
                    } else {
                        // Not ST — not a valid OSC, emit everything
                        output.extend_from_slice(&self.buf);
                        output.push(byte);
                        self.buf.clear();
                        self.state = StdinFilterState::Ground;
                    }
                }
            }
        }

        output
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
    /// Row offset for the initial phase after TUI handoff.
    /// When > 0, VT row N maps to real terminal row (N + 1 + row_offset)
    /// instead of (N + 1). Gradually consumed as VT content scrolls,
    /// or reset to 0 immediately on terminal resize or alternate screen.
    row_offset: u16,
    /// Number of usable content rows on the real terminal (excludes status line).
    /// Used to clip rendering so offset VT rows don't overwrite the status line.
    content_rows: u16,
}

impl Renderer {
    fn new(content_rows: u16) -> Self {
        Self {
            prev_lines: Vec::new(),
            prev_cursor: (0, 0, true),
            row_offset: 0,
            content_rows,
        }
    }

    /// Number of VT rows that fit on-screen given the current offset.
    fn visible_rows(&self) -> usize {
        (self.content_rows as usize).saturating_sub(self.row_offset as usize)
    }

    /// Scroll the real terminal by `count` lines within a temporary DECSTBM
    /// scroll region, pushing content into native scrollback while protecting
    /// the status line row.
    fn scroll_region(stdout: &mut impl Write, content_rows: u16, count: usize) -> io::Result<()> {
        if count == 0 || content_rows == 0 {
            return Ok(());
        }
        write!(stdout, "\x1b[1;{}r", content_rows)?;
        write!(stdout, "\x1b[{};1H", content_rows)?;
        stdout.write_all("\n".repeat(count).as_bytes())?;
        write!(stdout, "\x1b[r")
    }

    /// Render changed VT lines to stdout. Skips lines that haven't changed
    /// and clips rows that would fall outside the visible area.
    fn render(&mut self, stdout: &mut impl Write, vt: &Vt) -> io::Result<()> {
        let offset = self.row_offset as usize;
        let max_row = self.visible_rows();
        for (row_idx, line) in vt.view().enumerate() {
            if row_idx >= max_row {
                break;
            }
            let cells = line.cells();
            if row_idx < self.prev_lines.len() && cells == &self.prev_lines[row_idx][..] {
                continue;
            }
            write!(stdout, "\x1b[{};1H\x1b[2K", row_idx + 1 + offset)?;
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
    /// When the VT reports scrolled lines (via `Changes.scrollback`), we use a
    /// DECSTBM scroll region to protect the status line row, write newlines to scroll
    /// the real terminal, then reset the region. This pushes content into the terminal's
    /// native scrollback buffer — the same mechanism tmux uses.
    fn render_with_scroll(
        &mut self,
        stdout: &mut impl Write,
        vt: &Vt,
        scroll_count: usize,
    ) -> io::Result<()> {
        if scroll_count > 0 {
            let effective = scroll_count.min(self.content_rows as usize);
            Self::scroll_region(stdout, self.content_rows, effective)?;
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
        let offset = self.row_offset as usize;
        let max_row = self.visible_rows();
        self.prev_lines.clear();
        for (row_idx, line) in vt.view().enumerate() {
            if row_idx >= max_row {
                break;
            }
            write!(stdout, "\x1b[{};1H\x1b[2K", row_idx + 1 + offset)?;
            stdout.write_all(dump_line(line).as_bytes())?;
            write!(stdout, "\x1b[0m")?;
            self.prev_lines.push(line.cells().to_vec());
        }
        self.update_cursor(stdout, vt)
    }

    /// Position the real terminal cursor to match the VT cursor.
    fn update_cursor(&mut self, stdout: &mut impl Write, vt: &Vt) -> io::Result<()> {
        let offset = self.row_offset as usize;
        let cursor = vt.cursor();
        let new_cursor = (cursor.col, cursor.row, cursor.visible);
        if new_cursor != self.prev_cursor {
            if cursor.visible && !self.prev_cursor.2 {
                write!(stdout, "\x1b[?25h")?;
            } else if !cursor.visible && self.prev_cursor.2 {
                write!(stdout, "\x1b[?25l")?;
            }
            write!(
                stdout,
                "\x1b[{};{}H",
                cursor.row + 1 + offset,
                cursor.col + 1
            )?;
            self.prev_cursor = new_cursor;
        }
        Ok(())
    }

    /// Mark all lines as stale so the next render redraws everything.
    fn invalidate(&mut self) {
        self.prev_lines.clear();
    }

    /// Snapshot VT state into prev_lines without writing anything to stdout.
    /// Used after TUI handoff to establish a baseline for diff rendering
    /// while preserving existing terminal content.
    fn sync(&mut self, vt: &Vt) {
        self.prev_lines.clear();
        for line in vt.view() {
            self.prev_lines.push(line.cells().to_vec());
        }
        let cursor = vt.cursor();
        self.prev_cursor = (cursor.col, cursor.row, cursor.visible);
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
    Resize,
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
        let mut status_line = StatusLine::new();
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
        let (mut initial_cmd, _watch_files) = match command_rx.recv().await {
            Some(ShellCommand::Spawn {
                command,
                watch_files,
            }) => {
                self.status_line
                    .state_mut()
                    .set_watched_file_count(watch_files.len());
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

        // Create control FIFO for out-of-band signaling between shell and task runner.
        // Include timestamp nanos alongside PID to avoid stale file collisions
        // (e.g. after SIGKILL where the Drop handler does not run and a new
        // process reuses the same PID).
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let fifo_path =
            std::env::temp_dir().join(format!("devenv-ctrl-{}-{nanos}", std::process::id()));
        let control_pipe = ControlPipe::create(fifo_path).map_err(SessionError::Io)?;
        initial_cmd.env("DEVENV_CONTROL_FIFO", control_pipe.path());

        let pty = Arc::new(Pty::spawn(initial_cmd, pty_size)?);
        let mut vt = Vt::builder()
            .size(pty_size.cols as usize, pty_size.rows as usize)
            .scrollback_limit(self.size.rows as usize)
            .build();

        // Open the read end of the control pipe (non-blocking, async reads via epoll)
        let control_rx = control_pipe.into_receiver().map_err(SessionError::Io)?;

        // Handle TUI handoff if present
        if let Some(mut handoff) = handoff {
            // Run any tasks in the PTY (TUI still active, showing progress)
            if let Some(mut task_rx) = handoff.task_rx.take() {
                let mut task_runner = PtyTaskRunner::new(Arc::clone(&pty), control_rx);
                let pty_ready_tx = handoff.pty_ready_tx.take();
                task_runner
                    .run_with_vt(&mut task_rx, &mut vt, pty_ready_tx)
                    .await?;
            } else if let Some(tx) = handoff.pty_ready_tx.take() {
                let _ = tx.send(());
            }

            // Clear VT so internal task output (env exports, markers, drain
            // fences) is not rendered to the user.
            vt.feed_str("\x1b[2J\x1b[H");

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

        // TUI renderers may leave a non-default scroll region/origin mode.
        // Reset both before we start cursor-addressed rendering, otherwise
        // the first shell draw can land in the wrong area and overlap TUI output.
        write!(stdout, "\x1b[r\x1b[?6l")?;
        stdout.flush()?;

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
        // Both PTY and VT stay at full terminal size so that:
        // - Programs see the real dimensions (no unnecessary pager invocations)
        // - Alternate screen save/restore works correctly (same buffer size)
        // The renderer clips output to the visible area below cursor_row
        // and gradually consumes offset as the cursor moves down.
        let row_offset = cursor_row.saturating_sub(1);
        let pty_size = self.pty_size();
        let _ = pty.resize(pty_size);

        // Reset the VT after resize so any stale PTY output (the shell's
        // PROMPT_COMMAND after task execution, SIGWINCH redraw from the
        // resize above) starts on a clean slate. The event loop will
        // process any pending PTY output normally.
        vt.resize(pty_size.cols as usize, pty_size.rows as usize);
        vt.feed_str("\x1b[2J\x1b[H");

        // Initialize the renderer and do a full initial draw
        let mut renderer = Renderer::new(pty_size.rows);
        if row_offset > 0 {
            renderer.row_offset = row_offset;
            renderer.sync(&vt);
        } else {
            renderer.render_full(&mut stdout, &vt)?;
        }
        if self.config.show_status_line {
            self.status_line
                .draw(&mut stdout, self.size.cols, self.size.rows)?;
        }
        // Position cursor after initial draw
        let c = vt.cursor();
        let offset = renderer.row_offset as usize;
        write!(stdout, "\x1b[{};{}H", c.row + 1 + offset, c.col + 1)?;
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

        // Listen for SIGWINCH to handle terminal resize immediately
        #[cfg(unix)]
        {
            let resize_tx = event_tx_internal.clone();
            tokio::spawn(async move {
                let mut sigwinch =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change())
                        .expect("failed to register SIGWINCH handler");
                loop {
                    sigwinch.recv().await;
                    if resize_tx.send(Event::Resize).await.is_err() {
                        break;
                    }
                }
            });
        }

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
        let mut scanner = EscapeScanner::new();
        let mut stdin_filter = StdinFilter::new();
        let mut utf8_acc = Utf8Accumulator::new();
        let mut in_alternate_screen = false;
        // Track forwarded modes so we can clean them up on exit
        let mut forwarded_mouse_modes: Vec<u16> = Vec::new();
        let mut resize_pending = false;

        loop {
            // Use select! to handle both events and spinner animation
            let event = if resize_pending {
                resize_pending = false;
                Some(Event::Resize)
            } else if self.status_line.state().building {
                tokio::select! {
                    event = event_rx.recv() => event,
                    _ = tokio::time::sleep(spinner_interval) => {
                        if self.config.show_status_line {
                            stdout.write_all(b"\x1b[?2026h")?;
                            self.status_line.draw(stdout, self.size.cols, self.size.rows)?;
                            let c = vt.cursor();
                            let offset = renderer.row_offset as usize;
                            write!(stdout, "\x1b[{};{}H", c.row + 1 + offset, c.col + 1)?;
                            stdout.write_all(b"\x1b[?2026l")?;
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
                                let scroll_count = feed_vt(vt, &error_text);
                                if renderer.row_offset > 0 {
                                    renderer.render(stdout, vt)?;
                                } else {
                                    renderer.render_with_scroll(stdout, vt, scroll_count)?;
                                }
                            } else {
                                pty.write_all(&[0x0C])?;
                                pty.flush()?;
                            }
                            self.status_line
                                .draw(stdout, self.size.cols, self.size.rows)?;
                            let c = vt.cursor();
                            let offset = renderer.row_offset as usize;
                            write!(stdout, "\x1b[{};{}H", c.row + 1 + offset, c.col + 1)?;
                            stdout.flush()?;
                        }
                        continue;
                    }
                    let filtered = stdin_filter.filter(&data);
                    if !filtered.is_empty() {
                        pty.write_all(&filtered)?;
                        pty.flush()?;
                    }
                }

                Event::PtyOutput(data) => {
                    // Scan for DEC private mode sequences and forward them
                    let was_in_alt = in_alternate_screen;
                    let mut erase_display = false;
                    Self::process_escape_events(
                        &mut scanner,
                        &data,
                        &mut in_alternate_screen,
                        &mut forwarded_mouse_modes,
                        &mut erase_display,
                        stdout,
                    )?;

                    // Feed output into VT and track how many lines scrolled off
                    let mut total_scroll: usize = 0;
                    {
                        let text = utf8_acc.accumulate(&data);
                        total_scroll += feed_vt(vt, &text);
                    }

                    // Batch: drain any additional pending PtyOutput events
                    while let Ok(event) = event_rx.try_recv() {
                        match event {
                            Event::PtyOutput(more) => {
                                Self::process_escape_events(
                                    &mut scanner,
                                    &more,
                                    &mut in_alternate_screen,
                                    &mut forwarded_mouse_modes,
                                    &mut erase_display,
                                    stdout,
                                )?;
                                let text = utf8_acc.accumulate(&more);
                                total_scroll += feed_vt(vt, &text);
                            }
                            Event::PtyExit(exit_code) => {
                                Self::cleanup_forwarded_modes(
                                    in_alternate_screen,
                                    &forwarded_mouse_modes,
                                    stdout,
                                )?;
                                renderer.render_with_scroll(stdout, vt, total_scroll)?;
                                return Ok(exit_code);
                            }
                            Event::Stdin(stdin_data) => {
                                let filtered = stdin_filter.filter(&stdin_data);
                                if !filtered.is_empty() {
                                    pty.write_all(&filtered)?;
                                    pty.flush()?;
                                }
                            }
                            Event::Command(cmd) => {
                                total_scroll += self.handle_command(cmd, vt)?;
                            }
                            Event::Resize => {
                                resize_pending = true;
                                break;
                            }
                        }
                    }

                    // Begin synchronized output so the terminal buffers
                    // all writes atomically (mode 2026).
                    stdout.write_all(b"\x1b[?2026h")?;

                    // Handle alternate screen transitions
                    if was_in_alt != in_alternate_screen {
                        renderer.invalidate();
                    }

                    // Consume offset if needed: when cursor would land
                    // off-screen or VT scrolled, push old TUI content
                    // into native scrollback to make room.
                    if renderer.row_offset > 0 {
                        let content_rows = renderer.content_rows;
                        let visible_rows = renderer.visible_rows();
                        let cursor_excess = (vt.cursor().row + 1).saturating_sub(visible_rows);
                        let need = total_scroll.max(cursor_excess);

                        let consumed = if in_alternate_screen || erase_display {
                            // Alternate screen or explicit screen clear (CSI 2J):
                            // consume the entire offset so the shell owns the
                            // full visible area.
                            renderer.row_offset as usize
                        } else {
                            need.min(renderer.row_offset as usize)
                        };
                        if consumed > 0 {
                            Renderer::scroll_region(stdout, content_rows, consumed)?;
                            renderer.row_offset -= consumed as u16;
                            renderer.invalidate();
                        }
                    }

                    if in_alternate_screen || renderer.row_offset > 0 {
                        renderer.render(stdout, vt)?;
                    } else {
                        renderer.render_with_scroll(stdout, vt, total_scroll)?;
                    }

                    if self.config.show_status_line {
                        self.status_line
                            .draw(stdout, self.size.cols, self.size.rows)?;
                    }
                    let c = vt.cursor();
                    let offset = renderer.row_offset as usize;
                    write!(stdout, "\x1b[{};{}H", c.row + 1 + offset, c.col + 1)?;

                    // End synchronized output and flush.
                    stdout.write_all(b"\x1b[?2026l")?;
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
                    let scroll_count = self.handle_command(cmd, vt)?;
                    stdout.write_all(b"\x1b[?2026h")?;
                    if scroll_count > 0 {
                        if renderer.row_offset > 0 {
                            renderer.render(stdout, vt)?;
                        } else {
                            renderer.render_with_scroll(stdout, vt, scroll_count)?;
                        }
                    }
                    self.draw_status_and_cursor(stdout, vt, renderer)?;
                    stdout.write_all(b"\x1b[?2026l")?;
                    stdout.flush()?;
                }

                Event::Resize => {
                    if let Ok((cols, rows)) = terminal::size()
                        && (cols != self.size.cols || rows != self.size.rows)
                    {
                        self.size = PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        };
                        // Terminal resize ends the offset phase
                        renderer.row_offset = 0;
                        let pty_size = self.pty_size();
                        renderer.content_rows = pty_size.rows;
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
                                cols: pty_size.cols,
                                rows: pty_size.rows,
                            })
                            .await;
                    }
                }
            }
        }

        Self::cleanup_forwarded_modes(in_alternate_screen, &forwarded_mouse_modes, stdout)?;
        Ok(None)
    }

    /// Handle a command from the coordinator.
    ///
    /// Pure state update — no rendering. Returns the number of scrollback
    /// lines produced so the caller can fold it into its render pass.
    fn handle_command(&mut self, cmd: ShellCommand, vt: &mut Vt) -> Result<usize, SessionError> {
        match cmd {
            ShellCommand::ReloadReady { changed_files } => {
                self.status_line.state_mut().set_reload_ready(changed_files);
            }

            ShellCommand::Building { changed_files } => {
                self.status_line.state_mut().set_building(changed_files);
            }

            ShellCommand::BuildFailed {
                changed_files,
                error,
            } => {
                self.status_line
                    .state_mut()
                    .set_build_failed(changed_files, error);
            }

            ShellCommand::ReloadApplied => {
                self.status_line.state_mut().clear();
            }

            ShellCommand::WatchedFiles { files } => {
                self.status_line
                    .state_mut()
                    .set_watched_file_count(files.len());
            }

            ShellCommand::WatchingPaused { paused } => {
                self.status_line.state_mut().set_paused(paused);
            }

            ShellCommand::PrintWatchedFiles { files } => {
                let mut text = format!("\r\n\x1b[1mWatched files ({}):\x1b[0m\r\n", files.len());
                for file in &files {
                    text.push_str(&format!("  {}\r\n", file.display()));
                }
                return Ok(feed_vt(vt, &text));
            }

            ShellCommand::Shutdown => {
                // Will be handled by returning from event loop
            }

            ShellCommand::Spawn { .. } => {
                // Shouldn't receive Spawn after initial
            }
        }

        Ok(0)
    }

    /// Scan raw PTY output for escape sequences (DEC private mode and OSC queries),
    /// forward relevant ones to the real terminal, and update alternate screen state.
    ///
    /// Sets `*erase_display` to `true` if a CSI 2 J (erase display) sequence is
    /// detected so the caller can consume `row_offset` and let the shell clear
    /// content left by the TUI.
    fn process_escape_events(
        scanner: &mut EscapeScanner,
        data: &[u8],
        in_alternate_screen: &mut bool,
        forwarded_mouse_modes: &mut Vec<u16>,
        erase_display: &mut bool,
        stdout: &mut impl Write,
    ) -> io::Result<()> {
        for event in scanner.scan(data) {
            match event {
                SequenceEvent::DecMode(event) => {
                    if !event.has_forwarded_mode() {
                        continue;
                    }
                    stdout.write_all(event.raw_bytes())?;

                    if event.enters_alt_screen() {
                        *in_alternate_screen = true;
                    } else if event.exits_alt_screen() {
                        *in_alternate_screen = false;
                    }

                    match &event {
                        DecModeEvent::Set { modes, .. } => {
                            for &m in modes {
                                if matches!(
                                    m,
                                    1000 | 1002 | 1003 | 1005 | 1006 | 1015 | 2004 | 1004
                                ) && !forwarded_mouse_modes.contains(&m)
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
                SequenceEvent::Osc(event) => {
                    // Forward OSC queries to the real terminal so programs
                    // can detect color scheme, etc. The terminal's responses
                    // are filtered from stdin by StdinFilter to prevent them
                    // from leaking into the shell as garbled text.
                    stdout.write_all(&event.raw_bytes)?;
                }
                SequenceEvent::EraseDisplay { .. } => {
                    // CSI 2 J clears the entire display. Not forwarded (the
                    // renderer handles screen content), but we signal the
                    // caller so it can consume row_offset.
                    *erase_display = true;
                }
                SequenceEvent::ClearScrollback { raw_bytes } => {
                    stdout.write_all(&raw_bytes)?;
                }
                SequenceEvent::PrimaryDA { raw_bytes } => {
                    // Forward to the real terminal. The terminal's DA1 response
                    // arrives on stdin, passes through StdinFilter (it's CSI,
                    // not OSC), gets written to the PTY, and reaches the
                    // program that sent the query.
                    stdout.write_all(&raw_bytes)?;
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
    ///
    /// Does not flush — callers flush after ending their sync block.
    fn draw_status_and_cursor(
        &mut self,
        stdout: &mut Box<dyn Write + Send>,
        vt: &Vt,
        renderer: &Renderer,
    ) -> Result<(), SessionError> {
        if self.config.show_status_line {
            self.status_line
                .draw(stdout, self.size.cols, self.size.rows)?;
            let c = vt.cursor();
            let offset = renderer.row_offset as usize;
            write!(stdout, "\x1b[{};{}H", c.row + 1 + offset, c.col + 1)?;
        }
        Ok(())
    }
}

impl Default for ShellSession {
    fn default() -> Self {
        Self::with_defaults()
    }
}
