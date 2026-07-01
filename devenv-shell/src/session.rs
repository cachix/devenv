//! Shell session management.
//!
//! This module provides the main `ShellSession` type that orchestrates
//! PTY lifecycle, terminal I/O, and status line rendering.

use crate::escape::EscapeScanner;
use crate::escape_state::{
    EscapeState, cleanup_forwarded_modes as escape_state_cleanup,
    process_escape_events as escape_state_process,
};
use crate::protocol::{ShellCommand, ShellEvent};
use crate::pty::{Pty, PtyError, get_terminal_size};
use crate::status_line::{SPINNER_INTERVAL_MS, StatusLine};
use crate::terminal::RawModeGuard;
use crate::terminal_commands::{
    InBandResizeNotification, ORIGIN_MODE, ResetDecMode, ResetScrollRegion, SetScrollRegion,
};
use crate::utf8_accumulator::Utf8Accumulator;
use crate::vt_utils::{
    CursorState, DEFAULT_MAX_SCROLLBACK, active_point, cells_in_row, point_with_x, push_cell_text,
    screen_point,
};
use crossterm::{
    Command, cursor, queue,
    style::ResetColor,
    terminal::{self, Clear, ClearType},
};
use libghostty_vt::render::{Dirty, RenderState, RowIterator};
use libghostty_vt::screen::{Cell, CellContentTag, CellWide, GridRef};
use libghostty_vt::style::{Style, StyleColor, Underline};
use libghostty_vt::terminal::{Options as TerminalOptions, Point, Terminal};
use portable_pty::PtySize;
use std::fmt::Write as FmtWrite;
use std::io::{self, IsTerminal, Read, Write};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{mpsc as tokio_mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// Keybind byte sequences (ESC + Ctrl key).
const KEYBIND_TOGGLE_PAUSE: [u8; 2] = [0x1b, 0x04]; // Ctrl-Alt-D
const KEYBIND_LIST_WATCHED: [u8; 2] = [0x1b, 0x17]; // Ctrl-Alt-W
const KEYBIND_TOGGLE_ERROR: [u8; 2] = [0x1b, 0x05]; // Ctrl-Alt-E

fn dump_row_from_cells(buf: &mut String, vt: &Terminal<'_, '_>, point: Point, cells: &[Cell]) {
    // TODO(libghostty-rs): Style::is_default() returns true for RGB-bg-only styles
    // because StyleColor::Rgb is mistagged as NONE in the FFI From conversion.
    // Compare via PartialEq against Style::default() until upstream is fixed.
    let default_style = Style::default();
    let mut cur_style = default_style;
    let mut blank_cells: usize = 0;
    for (x, cell) in cells.iter().enumerate() {
        if matches!(
            cell.wide().ok(),
            Some(CellWide::SpacerTail | CellWide::SpacerHead)
        ) {
            continue;
        }
        let Ok(cell_ref) = vt.grid_ref(point_with_x(point, x as u16)) else {
            continue;
        };
        let has_text = cell.has_text().unwrap_or(false);
        let has_styling = cell.has_styling().unwrap_or(false);
        let tag = cell.content_tag().ok();
        let is_bg_only = matches!(
            tag,
            Some(CellContentTag::BgColorPalette | CellContentTag::BgColorRgb)
        );
        if !has_text && !has_styling && !is_bg_only {
            blank_cells += 1;
            continue;
        }
        if blank_cells > 0 {
            for _ in 0..blank_cells {
                buf.push(' ');
            }
            blank_cells = 0;
        }
        let new_style = cell_style(cell, &cell_ref, tag, has_styling, default_style);
        if new_style != cur_style {
            if new_style == default_style {
                buf.push_str("\x1b[0m");
            } else {
                dump_style(buf, &new_style);
            }
            cur_style = new_style;
        }
        push_cell_text(buf, cell, &cell_ref);
    }
    if cur_style != default_style {
        buf.push_str("\x1b[0m");
    }
}

fn cell_style(
    cell: &Cell,
    cell_ref: &GridRef<'_>,
    tag: Option<CellContentTag>,
    has_styling: bool,
    default: Style,
) -> Style {
    match tag {
        Some(CellContentTag::Codepoint | CellContentTag::CodepointGrapheme) => {
            if has_styling {
                cell_ref.style().unwrap_or(default)
            } else {
                default
            }
        }
        Some(CellContentTag::BgColorPalette) => {
            let mut s = default;
            if let Ok(idx) = cell.bg_color_palette() {
                s.bg_color = StyleColor::Palette(idx);
            }
            s
        }
        Some(CellContentTag::BgColorRgb) => {
            let mut s = default;
            if let Ok(rgb) = cell.bg_color_rgb() {
                s.bg_color = StyleColor::Rgb(rgb);
            }
            s
        }
        None => default,
    }
}

/// Render a VT row as a string with SGR escape sequences (fetches cells internally).
fn dump_row(buf: &mut String, vt: &Terminal<'_, '_>, point: Point) {
    let cells = cells_in_row(vt, point);
    dump_row_from_cells(buf, vt, point, &cells);
}

fn dump_style(s: &mut String, style: &Style) {
    s.push_str("\x1b[0");
    if style.fg_color != StyleColor::None {
        s.push(';');
        dump_color(s, &style.fg_color, 30);
    }
    if style.bg_color != StyleColor::None {
        s.push(';');
        dump_color(s, &style.bg_color, 40);
    }
    if style.bold {
        s.push_str(";1");
    }
    if style.faint {
        s.push_str(";2");
    }
    if style.italic {
        s.push_str(";3");
    }
    match style.underline {
        Underline::None => {}
        Underline::Single => s.push_str(";4"),
        Underline::Double => s.push_str(";4:2"),
        Underline::Curly => s.push_str(";4:3"),
        Underline::Dotted => s.push_str(";4:4"),
        Underline::Dashed => s.push_str(";4:5"),
        _ => {}
    }
    if style.blink {
        s.push_str(";5");
    }
    if style.inverse {
        s.push_str(";7");
    }
    if style.strikethrough {
        s.push_str(";9");
    }
    s.push('m');
}

fn dump_color(s: &mut String, color: &StyleColor, base: u8) {
    match color {
        StyleColor::Palette(p) if p.0 < 8 => {
            let _ = write!(s, "{}", base + p.0);
        }
        StyleColor::Palette(p) if p.0 < 16 => {
            let _ = write!(s, "{}", base + 52 + p.0);
        }
        StyleColor::Palette(p) => {
            let _ = write!(s, "{};5;{}", base + 8, p.0);
        }
        StyleColor::Rgb(rgb) => {
            let _ = write!(s, "{};2;{};{};{}", base + 8, rgb.r, rgb.g, rgb.b);
        }
        StyleColor::None => {}
    }
}

/// Feed text into VT and return the scroll count (lines that scrolled off the viewport).
///
/// Scroll count is the delta of `total_rows()`, which is exact as long as VT
/// scrollback hasn't hit its `max_scrollback` cap. We keep VT scrollback well
/// below the cap by wiping it via CSI 3J after every successful flush in
/// `render_with_scroll`, so the delta stays accurate in practice.
fn feed_vt(vt: &mut Terminal<'_, '_>, text: &str) -> usize {
    let total_before = vt.total_rows().unwrap_or(0);
    vt.vt_write(text.as_bytes());
    let total_after = vt.total_rows().unwrap_or(0);
    total_after.saturating_sub(total_before)
}

/// Differential renderer that draws VT state to a bounded terminal region.
///
/// Instead of passing raw PTY output to stdout (which conflicts with the status
/// line's scroll region), this renderer mediates all terminal output through
/// the VT state machine — similar to how tmux works.
struct Renderer {
    /// Previous frame for diffing — one line of cells per row.
    prev_lines: Vec<Vec<Cell>>,
    /// Previous cursor state.
    prev_cursor: CursorState,
    /// Row offset for the initial phase after TUI handoff.
    /// When > 0, VT row N maps to real terminal row (N + 1 + row_offset)
    /// instead of (N + 1). Gradually consumed as VT content scrolls,
    /// or reset to 0 immediately on terminal resize or alternate screen.
    row_offset: u16,
    /// Number of usable content rows on the real terminal (excludes status line).
    /// Used to clip rendering so offset VT rows don't overwrite the status line.
    content_rows: u16,
    /// Number of VT scrollback lines already pushed to native terminal scrollback.
    /// Used to flush only new (unflushed) scrollback lines in `render_with_scroll`.
    /// Reset to 0 after each flush.
    scrollback_flushed: usize,
    /// Reusable buffer for SGR line rendering (avoids per-line allocation).
    line_buf: String,
    /// libghostty render state. Snapshotted each `render` to read per-row dirty
    /// flags (so clean rows can be skipped) and to clear the terminal's dirty
    /// state. `None` if it could not be allocated, in which case `render` falls
    /// back to reading every row.
    render_state: Option<RenderState<'static>>,
    /// Reusable iterator over the render-state snapshot's rows, used to read and
    /// acknowledge each row's dirty flag. Paired with `render_state`; `None`
    /// disables dirty tracking and forces every row to be read.
    row_iter: Option<RowIterator<'static>>,
    /// Per-row dirty flags for the current frame, rebuilt by `refresh_dirty`
    /// from the render-state snapshot. Empty when dirty tracking is unavailable,
    /// in which case every row is treated as dirty.
    dirty_rows: Vec<bool>,
    /// Number of VT rows whose cells were read from the terminal across this
    /// renderer's lifetime. Used by tests to assert dirty-row tracking avoids
    /// re-reading unchanged rows.
    rows_read: u64,
}

impl Renderer {
    fn new(content_rows: u16) -> Self {
        let render_state = RenderState::new().ok();
        let row_iter = RowIterator::new().ok();
        if render_state.is_none() || row_iter.is_none() {
            tracing::debug!(
                "dirty-row tracking unavailable, renderer will read every row each frame"
            );
        }
        Self {
            prev_lines: Vec::new(),
            prev_cursor: CursorState {
                col: 0,
                row: 0,
                visible: true,
            },
            row_offset: 0,
            content_rows,
            scrollback_flushed: 0,
            line_buf: String::new(),
            render_state,
            row_iter,
            dirty_rows: Vec::new(),
            rows_read: 0,
        }
    }

    /// Discard any VT scrollback without emitting it to the native terminal
    /// (e.g., after resize where the post-resize state is the new baseline).
    fn discard_vt_scrollback(&mut self, vt: &mut Terminal<'_, '_>) {
        vt.vt_write(b"\x1b[3J");
        self.scrollback_flushed = 0;
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
        queue!(
            stdout,
            SetScrollRegion {
                top: 1,
                bottom: content_rows
            },
            cursor::MoveTo(0, content_rows - 1)
        )?;
        for _ in 0..count {
            stdout.write_all(b"\n")?;
        }
        queue!(stdout, ResetScrollRegion)
    }

    /// Write a single VT row's content (SGR-formatted text + reset) to stdout.
    fn write_row_content(
        &mut self,
        stdout: &mut impl Write,
        vt: &Terminal<'_, '_>,
        point: Point,
    ) -> io::Result<()> {
        self.line_buf.clear();
        dump_row(&mut self.line_buf, vt, point);
        stdout.write_all(self.line_buf.as_bytes())?;
        queue!(stdout, ResetColor)
    }

    /// Write a row using pre-fetched cells (avoids re-iterating cells via FFI).
    fn write_row_from_cells(
        &mut self,
        stdout: &mut impl Write,
        vt: &Terminal<'_, '_>,
        point: Point,
        cells: &[Cell],
    ) -> io::Result<()> {
        self.line_buf.clear();
        dump_row_from_cells(&mut self.line_buf, vt, point, cells);
        stdout.write_all(self.line_buf.as_bytes())?;
        queue!(stdout, ResetColor)
    }

    /// Rebuild `dirty_rows` from a fresh render-state snapshot and consume the
    /// terminal's dirty state in the same pass.
    ///
    /// The snapshot's per-row `dirty()` reflects changes the bare per-row bit
    /// (`grid_ref(...).row().is_dirty()`) misses — most importantly a viewport
    /// scroll, which rotates rows and marks dirtiness at the page level without
    /// setting each shifted row's own bit. `update` only snapshots the dirty
    /// state; it does not reset the render state's own per-row and global dirty
    /// layers, so we acknowledge each row (`set_dirty(false)`) and the global
    /// flag here, or every row would report dirty forever. Leaves `dirty_rows`
    /// empty when dirty tracking is unavailable, in which case `is_row_dirty`
    /// treats every row as dirty so `render` reads them all.
    fn refresh_dirty(&mut self, vt: &Terminal<'static, '_>) {
        let Self {
            render_state,
            row_iter,
            dirty_rows,
            ..
        } = self;
        dirty_rows.clear();
        let (Some(rs), Some(row_iter)) = (render_state, row_iter) else {
            return;
        };
        let snapshot = match rs.update(vt) {
            Ok(snapshot) => snapshot,
            Err(e) => {
                tracing::trace!(error = %e, "render-state update failed, reading all rows");
                return;
            }
        };
        {
            let mut iter = match row_iter.update(&snapshot) {
                Ok(iter) => iter,
                Err(e) => {
                    tracing::trace!(error = %e, "row-iterator update failed, reading all rows");
                    return;
                }
            };
            while let Some(row) = iter.next() {
                dirty_rows.push(row.dirty().unwrap_or(true));
                // If clearing the bit ever fails it stays set and the row
                // reports dirty forever, silently disabling the skip for it.
                if let Err(e) = row.set_dirty(false) {
                    tracing::trace!(error = %e, "failed to clear row dirty flag");
                }
            }
        }
        let _ = snapshot.set_dirty(Dirty::Clean);
    }

    /// Whether VT row `row_idx` changed since the last `render`.
    ///
    /// Returns `true` when dirty tracking is unavailable or the row is beyond
    /// the snapshot, so callers fall back to reading the row unconditionally.
    fn is_row_dirty(&self, row_idx: usize) -> bool {
        self.dirty_rows.get(row_idx).copied().unwrap_or(true)
    }

    /// Render changed VT lines to stdout. Skips lines that haven't changed
    /// and clips rows that would fall outside the visible area.
    ///
    /// Rows the terminal reports as clean are skipped without reading their
    /// cells at all, so a nested full-screen app (e.g. an editor) only costs
    /// per-cell FFI reads for the handful of rows it actually repaints rather
    /// than the whole screen every frame.
    fn render(&mut self, stdout: &mut impl Write, vt: &Terminal<'static, '_>) -> io::Result<()> {
        let offset = self.row_offset as usize;
        let max_row = self.visible_rows();
        let rows = vt.rows().unwrap_or(0) as usize;
        // Snapshot which rows changed since the last frame (and clear the
        // terminal's dirty state) before diffing, so clean rows can be skipped.
        self.refresh_dirty(vt);
        for row_idx in 0..rows.min(max_row) {
            // Skip clean rows we already have a baseline for: their cells cannot
            // have changed, so there's no need to read or diff them.
            if row_idx < self.prev_lines.len() && !self.is_row_dirty(row_idx) {
                continue;
            }
            let point = active_point(row_idx as u32);
            let cells = cells_in_row(vt, point);
            self.rows_read += 1;
            if row_idx < self.prev_lines.len() && cells == self.prev_lines[row_idx] {
                continue;
            }
            queue!(
                stdout,
                cursor::MoveTo(0, (row_idx + offset) as u16),
                Clear(ClearType::CurrentLine)
            )?;
            self.write_row_from_cells(stdout, vt, point, &cells)?;
            if row_idx >= self.prev_lines.len() {
                self.prev_lines.resize_with(row_idx + 1, Vec::new);
            }
            self.prev_lines[row_idx] = cells;
        }
        self.update_cursor(stdout, vt)
    }

    /// Push unflushed VT scrollback lines into native terminal scrollback,
    /// then render the viewport.
    ///
    /// Instead of blindly scrolling the previous screen content (which loses
    /// the actual scrolled-off text), this draws VT scrollback lines onto the
    /// real terminal and then scrolls them off via newlines inside a DECSTBM
    /// region that protects the status line.
    fn render_with_scroll(
        &mut self,
        stdout: &mut impl Write,
        vt: &mut Terminal<'static, '_>,
    ) -> io::Result<()> {
        let vt_scrollback = vt.scrollback_rows().unwrap_or(0);
        let unflushed = vt_scrollback.saturating_sub(self.scrollback_flushed);

        if unflushed > 0 && self.content_rows > 0 {
            let batch_size = self.content_rows as usize;

            // Set scroll region to protect the status line row.
            queue!(
                stdout,
                SetScrollRegion {
                    top: 1,
                    bottom: self.content_rows
                }
            )?;

            // Iterate scrollback lines starting from the first unflushed one.
            // Uses Screen coordinates where y=0 is the oldest scrollback line.
            let mut screen_y = self.scrollback_flushed;
            let mut remaining = unflushed;

            while remaining > 0 {
                let count = remaining.min(batch_size);
                let mut drawn = 0;

                // Pre-clear destination rows so soft-wrapped continuation rows
                // (which skip MoveTo+Clear below) land on clean rows.
                for i in 0..count {
                    queue!(
                        stdout,
                        cursor::MoveTo(0, i as u16),
                        Clear(ClearType::CurrentLine)
                    )?;
                }

                // After a soft-wrapped row, skip MoveTo so the outer terminal's
                // pending-wrap carries over instead of becoming a hard newline.
                let mut prev_was_wrap_source = false;

                for i in 0..count {
                    if screen_y >= vt_scrollback {
                        break;
                    }
                    let row_point = screen_point(screen_y as u32);
                    let row = vt.grid_ref(row_point).and_then(|gr| gr.row()).ok();
                    let is_continuation = row
                        .and_then(|r| r.is_wrap_continuation().ok())
                        .unwrap_or(false);
                    let is_wrap_source = row.and_then(|r| r.is_wrapped().ok()).unwrap_or(false);

                    if !(is_continuation && prev_was_wrap_source) {
                        queue!(stdout, cursor::MoveTo(0, i as u16))?;
                    }
                    self.write_row_content(stdout, vt, row_point)?;
                    screen_y += 1;
                    drawn += 1;
                    prev_was_wrap_source = is_wrap_source;
                }

                if drawn > 0 {
                    // Scroll drawn content into native scrollback.
                    queue!(stdout, cursor::MoveTo(0, self.content_rows - 1))?;
                    for _ in 0..drawn {
                        stdout.write_all(b"\n")?;
                    }
                }

                remaining -= count;
                if drawn < count {
                    break;
                }
            }

            queue!(stdout, ResetScrollRegion)?;

            // Wipe VT scrollback so it never approaches `max_scrollback` and
            // triggers internal GC. The user-visible scrollback lives in the
            // native terminal we just flushed to; VT scrollback is only an
            // internal staging area for not-yet-flushed lines. CSI 3J
            // (`EraseDisplay::scrollback`) preserves viewport, cursor, styles,
            // and modes — it only frees the history pages. See
            // `ghostty/src/terminal/Terminal.zig::eraseDisplay` and
            // `Screen.zig::eraseHistory`.
            vt.vt_write(b"\x1b[3J");
            self.scrollback_flushed = 0;
            self.prev_lines.clear();
        }
        self.render(stdout, vt)
    }

    /// Full redraw of all VT lines (after resize or initialization).
    fn render_full(
        &mut self,
        stdout: &mut impl Write,
        vt: &Terminal<'static, '_>,
    ) -> io::Result<()> {
        self.invalidate();
        self.render(stdout, vt)
    }

    /// Position the real terminal cursor to match the VT cursor.
    fn update_cursor(&mut self, stdout: &mut impl Write, vt: &Terminal<'_, '_>) -> io::Result<()> {
        let offset = self.row_offset as usize;
        let cur = CursorState::from_terminal(vt);
        if cur != self.prev_cursor {
            if cur.visible && !self.prev_cursor.visible {
                queue!(stdout, cursor::Show)?;
            } else if !cur.visible && self.prev_cursor.visible {
                queue!(stdout, cursor::Hide)?;
            }
            queue!(
                stdout,
                cursor::MoveTo(cur.col, (cur.row as usize + offset) as u16)
            )?;
            self.prev_cursor = cur;
        }
        Ok(())
    }

    /// Write the VT cursor position to stdout (unconditional, no diffing).
    ///
    /// Used to restore the real terminal cursor after status line draws
    /// or other operations that move it away from the VT position.
    fn write_cursor(&self, stdout: &mut impl Write, vt: &Terminal<'_, '_>) -> io::Result<()> {
        let cur = CursorState::from_terminal(vt);
        let offset = self.row_offset as usize;
        queue!(
            stdout,
            cursor::MoveTo(cur.col, (cur.row as usize + offset) as u16)
        )
    }

    /// Mark all lines as stale so the next render redraws everything.
    fn invalidate(&mut self) {
        self.prev_lines.clear();
    }

    /// Snapshot VT state into prev_lines without writing anything to stdout.
    /// Used after TUI handoff to establish a baseline for diff rendering
    /// while preserving existing terminal content.
    fn sync(&mut self, vt: &Terminal<'_, '_>) {
        self.prev_lines.clear();
        let rows = vt.rows().unwrap_or(0);
        for y in 0..rows {
            let cells = cells_in_row(vt, active_point(y as u32));
            self.prev_lines.push(cells);
        }
        self.prev_cursor = CursorState::from_terminal(vt);
    }
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("PTY error: {0}")]
    Pty(#[from] PtyError),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("terminal error: {0}")]
    Terminal(#[from] libghostty_vt::Error),
    #[error("channel closed")]
    ChannelClosed,
    #[error("unexpected command: expected Spawn, got {0}")]
    UnexpectedCommand(String),
}

/// Configuration for TUI handoff.
///
/// When running with TUI, the shell session needs to coordinate
/// terminal ownership with the TUI.
pub struct TuiHandoff {
    /// Signal the renderer to stop. Sending — or dropping — the sender is the
    /// signal (a closed channel is a delivered "stop", which makes the
    /// panic/early-return path safe without a guard).
    pub backend_done: oneshot::Sender<()>,
    /// Wait for the renderer to release the terminal. The TUI's final render
    /// height is carried but unused here.
    pub terminal_ready_rx: oneshot::Receiver<u16>,
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
/// Manages PTY lifecycle, terminal I/O, and status line rendering.
pub struct ShellSession {
    config: SessionConfig,
    size: PtySize,
    status_line: StatusLine,
    shutdown_token: Option<CancellationToken>,
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
            shutdown_token: None,
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

    /// Wire a shutdown token. On cancellation the session kills the inner
    /// shell so devenv can exit instead of orphaning it after a terminal
    /// hangup or SIGHUP/SIGINT/SIGTERM.
    pub fn with_shutdown_token(mut self, token: CancellationToken) -> Self {
        self.shutdown_token = Some(token);
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
        mut command_rx: tokio_mpsc::Receiver<ShellCommand>,
        event_tx: tokio_mpsc::Sender<ShellEvent>,
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
                    .set_watched_file_count(watch_files.len());
                (command, watch_files)
            }
            Some(ShellCommand::Shutdown) | None => {
                if let Some(h) = handoff {
                    let _ = h.backend_done.send(());
                }
                return Ok(None);
            }
            Some(other) => {
                if let Some(h) = handoff {
                    let _ = h.backend_done.send(());
                }
                return Err(SessionError::UnexpectedCommand(format!("{:?}", other)));
            }
        };

        // Spawn PTY
        // Reserve 1 row for status line if enabled
        let pty_size = self.pty_size();

        let pty = Arc::new(Pty::spawn(initial_cmd, pty_size)?);

        // TUI handoff. Wait for the renderer to release the terminal, but
        // yield to shutdown so a SIGHUP during this await can't hang us.
        if let Some(handoff) = handoff {
            tracing::trace!("session: sending backend_done");
            let _ = handoff.backend_done.send(());

            tracing::trace!("session: waiting for terminal_ready_rx");
            let cancelled = async {
                match &self.shutdown_token {
                    Some(t) => t.cancelled().await,
                    None => std::future::pending::<()>().await,
                }
            };
            tokio::select! {
                _ = handoff.terminal_ready_rx => {
                    tracing::trace!("session: terminal_ready_rx received");
                }
                _ = cancelled => {
                    tracing::debug!("session: shutdown during handoff, aborting");
                    let _ = pty.kill();
                    return Ok(None);
                }
            }
        }

        // Enter raw mode
        tracing::trace!("session: entering raw mode");
        let _raw_guard = RawModeGuard::new()?;
        tracing::trace!("session: raw mode active");

        let injected_stdin = io.stdin.is_some();
        let stdout_raw: Box<dyn Write + Send> = io.stdout.unwrap_or_else(|| Box::new(io::stdout()));
        let mut stdout: Box<dyn Write + Send> = Box::new(io::BufWriter::new(stdout_raw));
        let stdin_source: Box<dyn Read + Send> = io.stdin.unwrap_or_else(|| Box::new(io::stdin()));

        // Query cursor position FIRST before any terminal resets.
        // This tells us where TUI left the cursor after its final render.
        // Skip when stdin is injected (not a real terminal) — the response comes
        // via stdin, so this would hang if stdin is not a TTY.
        // crossterm::cursor::position() handles the DSR query, parsing, and has a
        // built-in 2s timeout for environments that don't respond (Docker, CI).
        let cursor_row = if !injected_stdin && io::stdin().is_terminal() {
            match crossterm::cursor::position() {
                Ok((_col, row)) => row + 1, // crossterm returns 0-based, we need 1-based
                Err(e) => {
                    tracing::debug!("session: cursor position query failed: {e}, assuming row 1");
                    1
                }
            }
        } else {
            1
        };
        tracing::trace!("session: cursor position after TUI: row {}", cursor_row);

        // TUI renderers may leave a non-default scroll region/origin mode.
        // Reset both before we start cursor-addressed rendering, otherwise
        // the first shell draw can land in the wrong area and overlap TUI output.
        queue!(stdout, ResetScrollRegion, ResetDecMode(ORIGIN_MODE))?;
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
        tracing::trace!(
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

        // Set up event channel
        let (event_tx_internal, event_rx_internal) = std::sync::mpsc::channel::<Event>();

        // On shutdown, kill the inner shell *and* inject a synthetic `PtyExit`:
        // if the child has already exited, `kill` returns ESRCH and on macOS
        // the PTY reader can stay blocked, so the event loop never sees the
        // real `PtyExit`. Signalled exit code is recovered upstream from
        // `Shutdown::last_signal()`.
        if let Some(token) = self.shutdown_token.clone() {
            let pty_killer = Arc::clone(&pty);
            let exit_tx = event_tx_internal.clone();
            tokio::spawn(async move {
                token.cancelled().await;
                tracing::debug!("session: shutdown requested, tearing down inner shell");
                if let Err(e) = pty_killer.kill() {
                    tracing::debug!("session: inner shell kill returned {e}");
                }
                let _ = exit_tx.send(Event::PtyExit(None));
            });
        }

        // Spawn stdin reader thread.
        let stdin_tx = event_tx_internal.clone();
        std::thread::Builder::new()
            .name("session-stdin".into())
            .spawn(move || {
                let mut stdin = stdin_source;
                let mut buf = [0u8; 1024];
                loop {
                    match stdin.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if stdin_tx.send(Event::Stdin(buf[..n].to_vec())).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("session: stdin read error: {}", e);
                            break;
                        }
                    }
                }
            })
            .expect("failed to spawn session-stdin thread");

        // Spawn PTY reader thread
        let pty_tx = event_tx_internal.clone();
        let pty_reader = Arc::clone(&pty);
        std::thread::Builder::new()
            .name("session-pty".into())
            .spawn(move || {
                // A large read buffer keeps a single PTY-output burst (e.g. a
                // full-screen TUI repaint) in one read so it feeds the VT and
                // renders as one frame instead of fragmenting across syscalls.
                let mut buf = [0u8; 65536];
                loop {
                    match pty_reader.read(&mut buf) {
                        Ok(0) => {
                            let exit_code =
                                pty_reader.try_wait().ok().flatten().map(|s| s.exit_code());
                            let _ = pty_tx.send(Event::PtyExit(exit_code));
                            break;
                        }
                        Ok(n) => {
                            if pty_tx.send(Event::PtyOutput(buf[..n].to_vec())).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("session: PTY read error: {}", e);
                            let exit_code =
                                pty_reader.try_wait().ok().flatten().map(|s| s.exit_code());
                            let _ = pty_tx.send(Event::PtyExit(exit_code));
                            break;
                        }
                    }
                }
            })
            .expect("failed to spawn session-pty thread");

        // Forward coordinator commands to internal event channel
        let cmd_tx = event_tx_internal.clone();
        tokio::spawn(async move {
            while let Some(cmd) = command_rx.recv().await {
                if cmd_tx.send(Event::Command(cmd)).is_err() {
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
                    if resize_tx.send(Event::Resize).is_err() {
                        break;
                    }
                }
            });
        }

        // Move VT processing and rendering to a dedicated thread.
        // Terminal is !Send, so all VT access must stay on one thread.
        let coordinator_tx = event_tx.clone();
        let pty_for_thread = Arc::clone(&pty);
        let vt_handle = std::thread::spawn(move || -> Result<Option<u32>, SessionError> {
            // Create the VT on this thread (Terminal is !Send)
            let mut vt = Terminal::new(TerminalOptions {
                cols: pty_size.cols,
                rows: pty_size.rows,
                max_scrollback: DEFAULT_MAX_SCROLLBACK,
            })?;

            // Reset the VT after resize so any stale PTY output (the shell's
            // PROMPT_COMMAND after task execution, SIGWINCH redraw from the
            // resize above) starts on a clean slate. The event loop will
            // process any pending PTY output normally.
            if let Err(e) = vt.resize(pty_size.cols, pty_size.rows, 0, 0) {
                tracing::warn!("failed to resize terminal: {e}");
            }
            vt.vt_write(b"\x1b[2J\x1b[H");

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
            renderer.write_cursor(&mut stdout, &vt)?;
            stdout.flush()?;

            self.event_loop(
                &pty_for_thread,
                &mut vt,
                &mut renderer,
                event_rx_internal,
                &coordinator_tx,
                &mut stdout,
            )
        });

        // Wait for VT thread without blocking the tokio runtime
        let exit_code = tokio::task::spawn_blocking(move || {
            vt_handle.join().unwrap_or(Err(SessionError::ChannelClosed))
        })
        .await
        .map_err(|_| SessionError::ChannelClosed)??;

        let _ = pty.kill();

        // Notify coordinator that shell exited
        if let Err(e) = event_tx.try_send(ShellEvent::Exited { exit_code }) {
            tracing::trace!("failed to send Exited event: {e}");
        }

        Ok(exit_code)
    }

    /// Main event loop handling stdin, PTY output, and coordinator commands.
    /// Returns the exit code from the PTY child process, if available.
    fn event_loop(
        &mut self,
        pty: &Arc<Pty>,
        vt: &mut Terminal<'static, '_>,
        renderer: &mut Renderer,
        event_rx: std::sync::mpsc::Receiver<Event>,
        coordinator_tx: &tokio_mpsc::Sender<ShellEvent>,
        stdout: &mut Box<dyn Write + Send>,
    ) -> Result<Option<u32>, SessionError> {
        let spinner_interval = Duration::from_millis(SPINNER_INTERVAL_MS);
        let mut scanner = EscapeScanner::new();
        let mut utf8_acc = Utf8Accumulator::new();
        let mut esc = EscapeState::new();
        let mut resize_pending = false;
        let mut esc_events = Vec::new();

        loop {
            // Use select! to handle both events and spinner animation
            let event = if resize_pending {
                resize_pending = false;
                Some(Event::Resize)
            } else if self.status_line.state().building {
                match event_rx.recv_timeout(spinner_interval) {
                    Ok(event) => Some(event),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if self.config.show_status_line {
                            queue!(stdout, terminal::BeginSynchronizedUpdate)?;
                            self.status_line
                                .draw(stdout, self.size.cols, self.size.rows)?;
                            renderer.write_cursor(stdout, vt)?;
                            queue!(stdout, terminal::EndSynchronizedUpdate)?;
                            stdout.flush()?;
                        }
                        continue;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => None,
                }
            } else if let Some(remaining) = self.status_line.state().reloaded_remaining() {
                match event_rx.recv_timeout(remaining) {
                    Ok(event) => Some(event),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        self.status_line.state_mut().clear_reloaded();
                        if self.config.show_status_line {
                            queue!(stdout, terminal::BeginSynchronizedUpdate)?;
                            self.status_line
                                .draw(stdout, self.size.cols, self.size.rows)?;
                            renderer.write_cursor(stdout, vt)?;
                            queue!(stdout, terminal::EndSynchronizedUpdate)?;
                            stdout.flush()?;
                        }
                        continue;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => None,
                }
            } else {
                event_rx.recv().ok()
            };

            let Some(event) = event else {
                break;
            };

            match event {
                Event::Stdin(data) => {
                    if data.as_slice() == KEYBIND_TOGGLE_PAUSE {
                        if let Err(e) = coordinator_tx.try_send(ShellEvent::TogglePause) {
                            tracing::trace!("failed to send TogglePause event: {e}");
                        }
                        continue;
                    }
                    if data.as_slice() == KEYBIND_LIST_WATCHED {
                        if let Err(e) = coordinator_tx.try_send(ShellEvent::ListWatchedFiles) {
                            tracing::trace!("failed to send ListWatchedFiles event: {e}");
                        }
                        continue;
                    }
                    if data.as_slice() == KEYBIND_TOGGLE_ERROR {
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
                                feed_vt(vt, &error_text);
                                if renderer.row_offset > 0 {
                                    renderer.render(stdout, vt)?;
                                } else {
                                    renderer.render_with_scroll(stdout, vt)?;
                                }
                            } else {
                                pty.write_all(&[0x0C])?;
                                pty.flush()?;
                            }
                            self.status_line
                                .draw(stdout, self.size.cols, self.size.rows)?;
                            renderer.write_cursor(stdout, vt)?;
                            stdout.flush()?;
                        }
                        continue;
                    }
                    if !&data.is_empty() {
                        pty.write_all(&data)?;
                        pty.flush()?;
                    }
                }

                Event::PtyOutput(data) => {
                    let was_in_alt = esc.in_alternate_screen;
                    esc.reset_batch();
                    escape_state_process(
                        &mut scanner,
                        &data,
                        &mut esc,
                        stdout,
                        pty,
                        self.pty_size(),
                        &mut esc_events,
                    )?;

                    // Feed output into VT and track how many lines scrolled off
                    let text = utf8_acc.accumulate(&data);
                    let mut total_scroll = feed_vt(vt, &text);

                    // Batch: drain any additional pending PtyOutput events
                    while let Ok(event) = event_rx.try_recv() {
                        match event {
                            Event::PtyOutput(more) => {
                                escape_state_process(
                                    &mut scanner,
                                    &more,
                                    &mut esc,
                                    stdout,
                                    pty,
                                    self.pty_size(),
                                    &mut esc_events,
                                )?;
                                let text = utf8_acc.accumulate(&more);
                                total_scroll += feed_vt(vt, &text);
                            }
                            Event::PtyExit(exit_code) => {
                                escape_state_cleanup(&esc, stdout)?;
                                renderer.render_with_scroll(stdout, vt)?;
                                return Ok(exit_code);
                            }
                            Event::Stdin(stdin_data) => {
                                if !&stdin_data.is_empty() {
                                    pty.write_all(&stdin_data)?;
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
                    queue!(stdout, terminal::BeginSynchronizedUpdate)?;

                    // Handle alternate screen transitions
                    if was_in_alt != esc.in_alternate_screen {
                        renderer.invalidate();
                    }

                    // Consume offset if needed: when cursor would land
                    // off-screen or VT scrolled, push old TUI content
                    // into native scrollback to make room.
                    if renderer.row_offset > 0 {
                        let content_rows = renderer.content_rows;
                        let visible_rows = renderer.visible_rows();
                        let cursor_row = vt.cursor_y().map(|r| r as usize).unwrap_or(0);
                        let cursor_excess = (cursor_row + 1).saturating_sub(visible_rows);
                        let need = total_scroll.max(cursor_excess);

                        let consumed = if esc.in_alternate_screen || esc.erase_display {
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

                    if esc.clear_scrollback {
                        queue!(stdout, Clear(ClearType::Purge))?;
                    }

                    if esc.in_alternate_screen || renderer.row_offset > 0 {
                        renderer.render(stdout, vt)?;
                    } else {
                        renderer.render_with_scroll(stdout, vt)?;
                    }

                    if self.config.show_status_line {
                        self.status_line
                            .draw(stdout, self.size.cols, self.size.rows)?;
                    }
                    renderer.write_cursor(stdout, vt)?;

                    // End synchronized output and flush.
                    queue!(stdout, terminal::EndSynchronizedUpdate)?;
                    stdout.flush()?;
                }

                Event::PtyExit(exit_code) => {
                    self.clear_status_row(stdout, esc.in_alternate_screen)?;
                    escape_state_cleanup(&esc, stdout)?;
                    stdout.flush()?;
                    return Ok(exit_code);
                }

                Event::Command(cmd) => {
                    self.handle_command(cmd, vt)?;
                    queue!(stdout, terminal::BeginSynchronizedUpdate)?;
                    if renderer.row_offset > 0 {
                        renderer.render(stdout, vt)?;
                    } else {
                        renderer.render_with_scroll(stdout, vt)?;
                    }
                    self.draw_status_and_cursor(stdout, vt, renderer)?;
                    queue!(stdout, terminal::EndSynchronizedUpdate)?;
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
                        // Send a mode 2048 in-band resize notification
                        // through the PTY, but only if the program has
                        // enabled mode 2048. Sending it unconditionally
                        // causes shells that don't understand it to display
                        // the raw escape sequence as input text.
                        if esc.in_band_resize {
                            let cmd = InBandResizeNotification {
                                rows: pty_size.rows,
                                cols: pty_size.cols,
                            };
                            let mut buf = String::new();
                            cmd.write_ansi(&mut buf).unwrap();
                            let _ = pty.write_all(buf.as_bytes());
                        }
                        if let Err(e) = vt.resize(pty_size.cols, pty_size.rows, 0, 0) {
                            tracing::warn!("failed to resize terminal: {e}");
                        }
                        renderer.discard_vt_scrollback(vt);
                        renderer.render_full(stdout, vt)?;
                        if self.config.show_status_line && !esc.in_alternate_screen {
                            self.status_line.draw(stdout, cols, rows)?;
                        }
                        renderer.write_cursor(stdout, vt)?;
                        stdout.flush()?;
                        if let Err(e) = coordinator_tx.try_send(ShellEvent::Resize {
                            cols: pty_size.cols,
                            rows: pty_size.rows,
                        }) {
                            tracing::trace!("failed to send Resize event: {e}");
                        }
                    }
                }
            }
        }

        self.clear_status_row(stdout, esc.in_alternate_screen)?;
        escape_state_cleanup(&esc, stdout)?;
        stdout.flush()?;
        Ok(None)
    }

    /// Handle a command from the coordinator.
    ///
    /// Updates state and, for some commands (e.g. `PrintWatchedFiles`), feeds
    /// text into the VT. Does not write to stdout. Returns the scroll count
    /// so the caller can fold it into its render pass.
    fn handle_command(
        &mut self,
        cmd: ShellCommand,
        vt: &mut Terminal<'_, '_>,
    ) -> Result<usize, SessionError> {
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
                self.status_line.state_mut().set_reloaded();
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

    /// Draw status line and reposition cursor.
    ///
    /// Does not flush — callers flush after ending their sync block.
    fn draw_status_and_cursor(
        &mut self,
        stdout: &mut impl Write,
        vt: &Terminal<'_, '_>,
        renderer: &Renderer,
    ) -> Result<(), SessionError> {
        if self.config.show_status_line {
            self.status_line
                .draw(stdout, self.size.cols, self.size.rows)?;
            renderer.write_cursor(stdout, vt)?;
        }
        Ok(())
    }

    /// Clear the status line row (e.g. on exit).
    fn clear_status_row(
        &self,
        stdout: &mut impl Write,
        in_alternate_screen: bool,
    ) -> io::Result<()> {
        if self.config.show_status_line && !in_alternate_screen {
            // Save cursor, clear the status row, restore cursor.
            queue!(
                stdout,
                cursor::SavePosition,
                cursor::MoveTo(0, self.size.rows - 1),
                Clear(ClearType::CurrentLine),
                cursor::RestorePosition,
            )?;
        }
        Ok(())
    }
}

impl Default for ShellSession {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portable_pty::CommandBuilder;

    fn make_vt(cols: u16, rows: u16) -> Terminal<'static, 'static> {
        Terminal::new(TerminalOptions {
            cols,
            rows,
            max_scrollback: DEFAULT_MAX_SCROLLBACK,
        })
        .expect("create terminal")
    }

    /// A nested full-screen app that repaints a single row must not cost a
    /// full-screen re-read. Dirty-row tracking should limit per-cell reads to
    /// the rows that actually changed, which is what keeps `devenv shell` fast
    /// when running editors like neovim on large terminals.
    #[test]
    fn render_reads_only_dirty_rows() {
        let rows: u16 = 50;
        let mut vt = make_vt(80, rows);
        let mut renderer = Renderer::new(rows);
        assert!(
            renderer.render_state.is_some(),
            "dirty tracking requires a render state"
        );
        let mut out: Vec<u8> = Vec::new();

        // Fill the screen, then draw the first frame.
        for i in 0..rows {
            vt.vt_write(format!("line {i}\r\n").as_bytes());
        }
        renderer.render(&mut out, &vt).unwrap();
        let full_reads = renderer.rows_read;
        assert!(
            full_reads >= rows as u64,
            "first frame should read every row, read {full_reads}"
        );

        // Repaint a single row in place, like a TUI updating one line.
        vt.vt_write(b"\x1b[10;1Hchanged");
        let before = renderer.rows_read;
        renderer.render(&mut out, &vt).unwrap();
        let delta = renderer.rows_read - before;

        assert!(
            delta < rows as u64,
            "second frame must not re-read the whole screen, read {delta} of {rows} rows"
        );
        // Only the edited row, plus at most the cursor's prior and landing
        // rows, should be re-read.
        assert!(
            delta <= 5,
            "expected only a handful of dirty rows, read {delta}"
        );
    }

    /// A frame where nothing changed must read no rows at all.
    #[test]
    fn render_skips_all_rows_when_clean() {
        let rows: u16 = 20;
        let mut vt = make_vt(80, rows);
        let mut renderer = Renderer::new(rows);
        let mut out: Vec<u8> = Vec::new();

        vt.vt_write(b"hello\r\nworld\r\n");
        renderer.render(&mut out, &vt).unwrap();

        let before = renderer.rows_read;
        renderer.render(&mut out, &vt).unwrap();
        assert_eq!(
            renderer.rows_read, before,
            "an unchanged frame must not read any rows"
        );
    }

    /// Scrolling a full-screen (alternate-screen) app shifts every row's
    /// on-screen content, but libghostty marks a scroll only at the page level,
    /// not via each row's own dirty bit. The renderer must read dirty state
    /// through the render-state snapshot (which reflects the page-level flag) so
    /// the shifted rows are redrawn; reading the bare row bit skipped them and
    /// left stale content on screen.
    #[test]
    fn render_redraws_scrolled_rows() {
        let rows: u16 = 6;
        let mut vt = make_vt(80, rows);
        let mut renderer = Renderer::new(rows);
        assert!(
            renderer.render_state.is_some(),
            "dirty tracking requires a render state"
        );
        let mut out: Vec<u8> = Vec::new();

        // A full-screen app on the alternate screen fills every row.
        vt.vt_write(b"\x1b[?1049h");
        vt.vt_write(b"L0\r\nL1\r\nL2\r\nL3\r\nL4\r\nL5");
        renderer.render(&mut out, &vt).unwrap();

        // Scroll up one line: row 0 now shows "L1", ..., row 4 shows "L5", and a
        // new bottom line "L6" appears. Every row's content changed.
        vt.vt_write(b"\r\nL6");
        let before = renderer.rows_read;
        let mut out2: Vec<u8> = Vec::new();
        renderer.render(&mut out2, &vt).unwrap();
        let frame = String::from_utf8_lossy(&out2);

        for line in ["L1", "L2", "L3", "L4", "L5", "L6"] {
            assert!(
                frame.contains(line),
                "scrolled row {line:?} was not redrawn (stale): {frame:?}"
            );
        }
        // Every row's content shifted, so the snapshot must mark all of them
        // dirty and `render` must re-read each one. If dirty came from the bare
        // per-row bit instead of the page-level snapshot flag, the shifted rows
        // would be skipped without reading (delta < rows) and left stale — this
        // assertion, not the substring check above, is what guards the
        // page-level dirty path (a plain content-diff redraw would also satisfy
        // the substring check).
        let delta = renderer.rows_read - before;
        assert_eq!(
            delta, rows as u64,
            "every scrolled row must be re-read via the snapshot dirty flag, read {delta} of {rows}"
        );
    }

    /// Regression test for devenv#2845: when the process-wide shutdown token is
    /// cancelled (e.g. from the SIGHUP/SIGINT/SIGTERM handler), the inner shell
    /// must die with it. Otherwise the PTY (in its own session via setsid)
    /// outlives devenv and orphans, burning CPU after the terminal closes.
    ///
    /// Exercises the same wiring `ShellSession::run` installs after PTY spawn:
    /// a tokio task that, on `token.cancelled()`, calls `pty.kill()`.
    #[tokio::test(flavor = "multi_thread")]
    async fn shutdown_token_kills_inner_shell() {
        let mut cmd = CommandBuilder::new("sleep");
        cmd.arg("5");
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pty = Arc::new(Pty::spawn(cmd, size).expect("spawn inner pty"));

        let token = CancellationToken::new();
        let pty_killer = Arc::clone(&pty);
        let token_for_task = token.clone();
        tokio::spawn(async move {
            token_for_task.cancelled().await;
            let _ = pty_killer.kill();
        });

        token.cancel();

        // The kill is asynchronous; poll briefly for the child to reap.
        let mut status = None;
        for _ in 0..500 {
            status = pty.try_wait().expect("try_wait");
            if status.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            status.is_some(),
            "inner shell still running after shutdown token cancellation"
        );
    }
}
