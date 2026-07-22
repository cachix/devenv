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
    CursorState, DEFAULT_MAX_SCROLLBACK, active_point, point_with_x, push_cell_text, screen_point,
};
use crossterm::{
    Command, cursor, queue,
    terminal::{self, Clear, ClearType},
};
use libghostty_vt::render::{CellIterator, Dirty, RenderState, RowIteration, RowIterator};
use libghostty_vt::screen::{Cell, CellContentTag, CellWide, GridRef, Screen, TrackedGridRef};
use libghostty_vt::style::{Style, StyleColor, Underline};
use libghostty_vt::terminal::{Mode, Options as TerminalOptions, Point, PointSpace, Terminal};
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

/// Fetch all cells in a VT row into a reusable buffer.
fn cells_in_row_into(vt: &Terminal<'_, '_>, point: Point, cells: &mut Vec<Cell>) {
    cells.clear();
    let cols = vt.cols().unwrap_or(0);
    cells.extend((0..cols).filter_map(|x| vt.grid_ref(point_with_x(point, x)).ok()?.cell().ok()));
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
    if style.underline_color != StyleColor::None {
        s.push_str(";58");
        dump_extended_color(s, &style.underline_color);
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
    if style.invisible {
        s.push_str(";8");
    }
    if style.strikethrough {
        s.push_str(";9");
    }
    if style.overline {
        s.push_str(";53");
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

fn dump_extended_color(s: &mut String, color: &StyleColor) {
    match color {
        StyleColor::Palette(p) => {
            let _ = write!(s, ";5;{}", p.0);
        }
        StyleColor::Rgb(rgb) => {
            let _ = write!(s, ";2;{};{};{}", rgb.r, rgb.g, rgb.b);
        }
        StyleColor::None => {}
    }
}

/// Render a row from Ghostty's render-state snapshot. Unlike `GridRef`, the
/// cell iterator is intended for render loops and provides style and grapheme
/// data without resolving every cell against the terminal a second time.
fn dump_row_from_render_state<'a>(
    buf: &mut String,
    row: &RowIteration<'a, '_>,
    cell_iter: &mut CellIterator<'a>,
    cells_buf: &mut Vec<Cell>,
    grapheme_buf: &mut Vec<char>,
) -> Result<(), libghostty_vt::Error> {
    cells_buf.clear();
    // Ghostty tracks whether a row contains any ref-counted styles. The flag
    // may have false positives but no false negatives, so plain-text rows can
    // skip one FFI query per cell without missing styled content. Background-
    // only cells are stored in the content tag and remain handled below.
    let row_styled = row.raw_row()?.is_styled()?;
    let default_style = Style::default();
    let mut cur_style = default_style;
    let mut blank_cells = 0usize;
    let mut cells = cell_iter.update(row)?;

    while let Some(render_cell) = cells.next() {
        let cell = render_cell.raw_cell()?;
        cells_buf.push(cell);
        if matches!(cell.wide()?, CellWide::SpacerTail | CellWide::SpacerHead) {
            continue;
        }

        let tag = cell.content_tag()?;
        // Derive text presence from data that encoding needs anyway. This
        // avoids a separate `has_text` FFI call for every cell.
        let mut codepoint = None;
        let mut grapheme_len = 0;
        let has_text = match tag {
            CellContentTag::Codepoint => {
                let cp = cell.codepoint()?;
                codepoint = Some(cp);
                cp != 0
            }
            CellContentTag::CodepointGrapheme => {
                grapheme_len = render_cell.graphemes_len()?;
                grapheme_len > 0
            }
            CellContentTag::BgColorPalette | CellContentTag::BgColorRgb => false,
        };
        let has_styling = row_styled && cell.has_styling()?;
        let is_bg_only = matches!(
            tag,
            CellContentTag::BgColorPalette | CellContentTag::BgColorRgb
        );
        if !has_text && !has_styling && !is_bg_only {
            blank_cells += 1;
            continue;
        }
        if blank_cells > 0 {
            buf.extend(std::iter::repeat_n(' ', blank_cells));
            blank_cells = 0;
        }

        let new_style = match tag {
            CellContentTag::Codepoint | CellContentTag::CodepointGrapheme => {
                if has_styling {
                    render_cell.style().unwrap_or(default_style)
                } else {
                    default_style
                }
            }
            CellContentTag::BgColorPalette => {
                let mut style = default_style;
                if let Ok(idx) = cell.bg_color_palette() {
                    style.bg_color = StyleColor::Palette(idx);
                }
                style
            }
            CellContentTag::BgColorRgb => {
                let mut style = default_style;
                if let Ok(rgb) = cell.bg_color_rgb() {
                    style.bg_color = StyleColor::Rgb(rgb);
                }
                style
            }
        };
        if new_style != cur_style {
            if new_style == default_style {
                buf.push_str("\x1b[0m");
            } else {
                dump_style(buf, &new_style);
            }
            cur_style = new_style;
        }

        match tag {
            CellContentTag::Codepoint if has_text => {
                if let Some(ch) = codepoint.and_then(char::from_u32) {
                    buf.push(ch);
                }
            }
            CellContentTag::CodepointGrapheme if has_text => {
                grapheme_buf.resize(grapheme_len, '\0');
                render_cell.graphemes_buf(grapheme_buf)?;
                buf.extend(grapheme_buf.iter().copied());
            }
            _ => buf.push(' '),
        }
    }
    Ok(())
}

fn collect_render_state_cells<'a>(
    row: &RowIteration<'a, '_>,
    cell_iter: &mut CellIterator<'a>,
    cells_buf: &mut Vec<Cell>,
) -> Result<(), libghostty_vt::Error> {
    cells_buf.clear();
    let mut cells = cell_iter.update(row)?;
    while let Some(cell) = cells.next() {
        cells_buf.push(cell.raw_cell()?);
    }
    Ok(())
}

fn store_row_cells(prev_lines: &mut Vec<Vec<Cell>>, row_idx: usize, cells_buf: &mut Vec<Cell>) {
    if row_idx >= prev_lines.len() {
        let row_capacity = cells_buf.capacity();
        prev_lines.resize_with(row_idx + 1, || Vec::with_capacity(row_capacity));
    }
    std::mem::swap(&mut prev_lines[row_idx], cells_buf);
    cells_buf.clear();
}

/// Write a single VT row (SGR-formatted, from pre-fetched cells) to stdout.
fn queue_row_from_cells(
    stdout: &mut impl Write,
    vt: &Terminal<'_, '_>,
    point: Point,
    cells: &[Cell],
    line_buf: &mut String,
) -> io::Result<()> {
    line_buf.clear();
    dump_row_from_cells(line_buf, vt, point, cells);
    line_buf.push_str("\x1b[0m");
    stdout.write_all(line_buf.as_bytes())?;
    Ok(())
}

/// Fetch a VT row's cells, and redraw it at `row_idx + offset` if they differ
/// from the `prev_lines` baseline. Returns whether the row was drawn.
#[expect(clippy::too_many_arguments)]
fn draw_row_if_changed(
    stdout: &mut impl Write,
    vt: &Terminal<'_, '_>,
    row_idx: usize,
    offset: usize,
    baseline_valid: bool,
    prev_lines: &mut Vec<Vec<Cell>>,
    cells_buf: &mut Vec<Cell>,
    line_buf: &mut String,
) -> io::Result<bool> {
    let point = active_point(row_idx as u32);
    cells_in_row_into(vt, point, cells_buf);
    if baseline_valid && row_idx < prev_lines.len() && *cells_buf == prev_lines[row_idx] {
        return Ok(false);
    }
    queue!(
        stdout,
        cursor::MoveTo(0, (row_idx + offset) as u16),
        Clear(ClearType::CurrentLine)
    )?;
    queue_row_from_cells(stdout, vt, point, cells_buf, line_buf)?;
    store_row_cells(prev_lines, row_idx, cells_buf);
    Ok(true)
}

/// Draw a viewport row from Ghostty's render-state snapshot. `None` means the
/// snapshot iterator failed and the caller should use the GridRef fallback.
#[expect(clippy::too_many_arguments)]
fn draw_render_state_row_if_changed<'a>(
    stdout: &mut impl Write,
    row: &RowIteration<'a, '_>,
    cell_iter: &mut CellIterator<'a>,
    compare_cell_iter: &mut CellIterator<'a>,
    row_idx: usize,
    offset: usize,
    baseline_valid: bool,
    compare_first: bool,
    prev_lines: &mut Vec<Vec<Cell>>,
    cells_buf: &mut Vec<Cell>,
    grapheme_buf: &mut Vec<char>,
    line_buf: &mut String,
) -> io::Result<Option<bool>> {
    // Untrusted dirty flags (scrolling and alternate screens) can make every
    // row a candidate. Compare raw cells first there so clean rows don't pay
    // the higher cost of style and grapheme encoding. A separate iterator is
    // required because a second update of the same cell iterator does not
    // rewind the current row in all libghostty versions.
    if compare_first {
        if collect_render_state_cells(row, compare_cell_iter, cells_buf).is_err() {
            return Ok(None);
        }
        if row_idx < prev_lines.len() && *cells_buf == prev_lines[row_idx] {
            return Ok(Some(false));
        }
    }

    line_buf.clear();
    if dump_row_from_render_state(line_buf, row, cell_iter, cells_buf, grapheme_buf).is_err() {
        return Ok(None);
    }
    if baseline_valid && row_idx < prev_lines.len() && *cells_buf == prev_lines[row_idx] {
        return Ok(Some(false));
    }
    line_buf.push_str("\x1b[0m");
    queue!(
        stdout,
        cursor::MoveTo(0, (row_idx + offset) as u16),
        Clear(ClearType::CurrentLine)
    )?;
    stdout.write_all(line_buf.as_bytes())?;
    store_row_cells(prev_lines, row_idx, cells_buf);
    Ok(Some(true))
}

#[derive(Debug, Default)]
struct VtInputFilter {
    state: VtInputFilterState,
    pending: Vec<u8>,
}

#[derive(Debug, Default)]
enum VtInputFilterState {
    #[default]
    Ground,
    Esc,
    TmuxTitle,
    TmuxTitleEsc,
}

impl VtInputFilter {
    fn new() -> Self {
        Self {
            state: VtInputFilterState::Ground,
            pending: Vec::new(),
        }
    }

    fn filter<'a>(&mut self, data: &'a [u8], output: &'a mut Vec<u8>) -> &'a [u8] {
        output.clear();

        for &byte in data {
            match self.state {
                VtInputFilterState::Ground => {
                    if byte == 0x1b {
                        self.pending.clear();
                        self.pending.push(byte);
                        self.state = VtInputFilterState::Esc;
                    } else {
                        output.push(byte);
                    }
                }

                VtInputFilterState::Esc => {
                    self.pending.push(byte);
                    if byte == b'k' {
                        self.pending.clear();
                        self.state = VtInputFilterState::TmuxTitle;
                    } else if byte == 0x1b {
                        self.pending.clear();
                        self.pending.push(byte);
                    } else {
                        output.extend_from_slice(&self.pending);
                        self.pending.clear();
                        self.state = VtInputFilterState::Ground;
                    }
                }

                VtInputFilterState::TmuxTitle => {
                    if byte == 0x1b {
                        self.state = VtInputFilterState::TmuxTitleEsc;
                    }
                }

                VtInputFilterState::TmuxTitleEsc => {
                    if byte == b'\\' {
                        self.state = VtInputFilterState::Ground;
                    } else if byte == 0x1b {
                        self.state = VtInputFilterState::Esc;
                        self.pending.clear();
                        self.pending.push(byte);
                    } else {
                        self.state = VtInputFilterState::Ground;
                        output.push(0x1b);
                        output.push(byte);
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
fn primary_height_shrunk(
    old_native_rows: u16,
    new_native_rows: u16,
    active: Option<Screen>,
) -> bool {
    new_native_rows < old_native_rows && active == Some(Screen::Primary)
}

/// Whether the PTY application has asked us to defer presentation with
/// synchronized output (DEC mode 2026). While this is active, emitting our
/// own end marker would prematurely expose an intermediate frame.
fn synchronized_output_active(vt: &Terminal<'_, '_>) -> bool {
    vt.mode(Mode::SYNC_OUTPUT).unwrap_or(false)
}

struct Renderer<'a> {
    /// Previous frame for diffing — one line of cells per row.
    prev_lines: Vec<Vec<Cell>>,
    /// Whether every visible entry in `prev_lines` is a valid baseline.
    /// Invalidating keeps the allocated row buffers available for reuse.
    prev_lines_valid: bool,
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
    /// Tracked grid ref (ghostty pin) at the first VT row not yet flushed to
    /// native terminal scrollback. Ghostty keeps it anchored to that row
    /// across scrolling, history pruning, and resize reflow, so flush
    /// accounting stays exact without wiping VT history.
    /// `None` only if pin allocation or re-anchoring failed. In that case,
    /// `flush_boundary_rows` keeps numeric accounting until a new pin can be
    /// allocated.
    flush_boundary: Option<TrackedGridRef>,
    /// Numeric screen-row fallback for `flush_boundary`. This is only used
    /// when a tracked reference cannot be allocated or re-anchored.
    flush_boundary_rows: Option<usize>,
    /// During a primary-screen height shrink, the first row moved from the
    /// old viewport into history. Those rows already exist in the native
    /// terminal, so they must not be emitted by the history flush.
    resize_flush_end: Option<TrackedGridRef>,
    /// Numeric fallback for `resize_flush_end` when its pin cannot be made.
    resize_flush_end_rows: Option<usize>,
    /// Viewport lines scrolled off since the last render; tells `render` that
    /// row content shifted so per-row dirty flags alone can't be trusted.
    pending_scroll: usize,
    /// Ghostty render state for dirty tracking (which rows changed).
    render_state: RenderState<'a>,
    /// Reusable row iterator over `render_state` snapshots.
    row_iter: RowIterator<'a>,
    /// Reusable cell iterator over a render-state row.
    cell_iter: CellIterator<'a>,
    /// Independent iterator used for cheap compare-before-encode passes.
    compare_cell_iter: CellIterator<'a>,
    /// Reusable cell scratch space for allocation-free row comparisons.
    cells_buf: Vec<Cell>,
    /// Reusable scratch space for multi-codepoint grapheme clusters.
    grapheme_buf: Vec<char>,
    /// Reusable buffer for SGR line rendering (avoids per-line allocation).
    line_buf: String,
}

impl<'a> Renderer<'a> {
    fn new(content_rows: u16, vt: &Terminal<'a, '_>) -> Result<Self, libghostty_vt::Error> {
        let flush_boundary = vt.track_grid_ref(active_point(0)).ok();
        if flush_boundary.is_none() {
            tracing::warn!("failed to create flush boundary pin, using numeric scroll accounting");
        }
        let cols = vt.cols().unwrap_or(0) as usize;
        let rows = vt.rows().unwrap_or(0) as usize;
        Ok(Self {
            prev_lines: (0..rows).map(|_| Vec::with_capacity(cols)).collect(),
            prev_lines_valid: false,
            prev_cursor: CursorState {
                col: 0,
                row: 0,
                visible: true,
            },
            row_offset: 0,
            content_rows,
            flush_boundary,
            flush_boundary_rows: None,
            resize_flush_end: None,
            resize_flush_end_rows: None,
            pending_scroll: 0,
            render_state: RenderState::new()?,
            row_iter: RowIterator::new()?,
            cell_iter: CellIterator::new()?,
            compare_cell_iter: CellIterator::new()?,
            cells_buf: Vec::with_capacity(cols),
            grapheme_buf: Vec::new(),
            line_buf: String::with_capacity(cols * 4),
        })
    }

    /// Feed text into the VT and return the scroll count (lines that scrolled
    /// off the viewport), measured as growth of the unflushed region so it
    /// stays exact even when ghostty prunes old history pages.
    fn feed(&mut self, vt: &mut Terminal<'_, '_>, text: &str) -> usize {
        self.normalize_fallback_boundary(vt.scrollback_rows().unwrap_or(0));
        let before = self.unflushed(vt);
        vt.vt_write(text.as_bytes());
        let scrolled = self.unflushed(vt).saturating_sub(before);
        self.pending_scroll += scrolled;
        scrolled
    }

    /// Keep numeric fallback accounting valid when Ghostty prunes old history.
    fn normalize_fallback_boundary(&mut self, scrollback: usize) {
        if let Some(boundary) = &mut self.flush_boundary_rows {
            *boundary = (*boundary).min(scrollback);
        }
    }

    /// Number of VT scrollback rows not yet flushed to the native terminal:
    /// the distance from the flush boundary pin to the active area.
    ///
    /// Returns 0 on the alternate screen (it has no scrollback).
    fn unflushed(&self, vt: &Terminal<'_, '_>) -> usize {
        if vt.active_screen().ok() != Some(Screen::Primary) {
            return 0;
        }
        let scrollback = vt.scrollback_rows().unwrap_or(0);
        let boundary_y = self
            .flush_boundary
            .as_ref()
            .and_then(|b| b.point(PointSpace::Screen).ok().flatten())
            .map(|p| p.y as usize)
            .or(self.flush_boundary_rows)
            .unwrap_or(0);
        let end = self
            .resize_flush_end_rows
            .or_else(|| {
                self.resize_flush_end
                    .as_ref()
                    .and_then(|b| b.point(PointSpace::Screen).ok().flatten())
                    .map(|p| p.y as usize)
            })
            .unwrap_or(scrollback)
            .min(scrollback);
        end.saturating_sub(boundary_y.min(end))
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
        cells_in_row_into(vt, point, &mut self.cells_buf);
        queue_row_from_cells(stdout, vt, point, &self.cells_buf, &mut self.line_buf)
    }

    /// Render changed VT lines to stdout. Skips lines that haven't changed
    /// and clips rows that would fall outside the visible area.
    ///
    /// Uses ghostty's render-state dirty tracking to skip clean rows without
    /// fetching their cells. Dirty flags are only a skip-hint: rows they
    /// mark dirty are still diffed against `prev_lines` before drawing, and
    /// they are bypassed entirely (full fetch + diff) when the baseline is
    /// incomplete, when content scrolled, or on the alternate screen.
    fn render(&mut self, stdout: &mut impl Write, vt: &Terminal<'a, '_>) -> io::Result<()> {
        let offset = self.row_offset as usize;
        let max_row = self.visible_rows();
        let rows = vt.rows().unwrap_or(0) as usize;
        let visible = rows.min(max_row);
        let scrolled = self.pending_scroll > 0;
        self.pending_scroll = 0;
        // Scrolling shifts every row's content, and on the alternate screen
        // scrolls may not mark rows dirty, so trust dirty flags only on a
        // scroll-free primary-screen frame.
        let trust_dirty = !scrolled && vt.active_screen().ok() == Some(Screen::Primary);
        let baseline_valid = self.prev_lines_valid && self.prev_lines.len() >= visible;

        let Self {
            render_state,
            row_iter,
            cell_iter,
            compare_cell_iter,
            prev_lines,
            cells_buf,
            grapheme_buf,
            line_buf,
            ..
        } = self;

        // Dirty-tracking fast path; falls back to a full diff pass on any
        // render-state failure.
        let mut dirty_pass_done = false;
        match render_state.update(vt) {
            Ok(snapshot) => {
                let dirty = snapshot.dirty().unwrap_or(Dirty::Full);
                let clean = dirty == Dirty::Clean;
                // Ghostty also reports Full when the viewport pin moves. Our
                // scroll path has already rotated the native-output baseline,
                // so retain its compare-before-encode path for those frames.
                // Other global changes still require a forced row rebuild.
                let force_full = dirty == Dirty::Full && !scrolled;
                if trust_dirty && clean && baseline_valid {
                    dirty_pass_done = true;
                } else if let Ok(mut iteration) = row_iter.update(&snapshot) {
                    let mut row_idx = 0usize;
                    while let Some(row) = iteration.next() {
                        if row_idx < visible {
                            let must_fetch = force_full
                                || !trust_dirty
                                || !baseline_valid
                                || row.dirty().unwrap_or(true);
                            if must_fetch {
                                // A full dirty state represents global changes
                                // (screen, viewport, dimensions, or terminal
                                // state), so raw-cell equality is insufficient.
                                // Ghostty's renderer rebuilds every row here.
                                let row_baseline_valid = baseline_valid && !force_full;
                                let drawn = draw_render_state_row_if_changed(
                                    stdout,
                                    row,
                                    cell_iter,
                                    compare_cell_iter,
                                    row_idx,
                                    offset,
                                    row_baseline_valid,
                                    !force_full && !trust_dirty && baseline_valid,
                                    prev_lines,
                                    cells_buf,
                                    grapheme_buf,
                                    line_buf,
                                )?;
                                if drawn.is_none() {
                                    draw_row_if_changed(
                                        stdout,
                                        vt,
                                        row_idx,
                                        offset,
                                        row_baseline_valid,
                                        prev_lines,
                                        cells_buf,
                                        line_buf,
                                    )?;
                                }
                            }
                        }
                        let _ = row.set_dirty(false);
                        row_idx += 1;
                    }
                    dirty_pass_done = row_idx >= visible;
                }
                let _ = snapshot.set_dirty(Dirty::Clean);
            }
            Err(e) => {
                tracing::debug!(error = %e, "render state update failed");
            }
        }
        if !dirty_pass_done {
            for row_idx in 0..visible {
                draw_row_if_changed(
                    stdout,
                    vt,
                    row_idx,
                    offset,
                    baseline_valid,
                    prev_lines,
                    cells_buf,
                    line_buf,
                )?;
            }
        }
        self.prev_lines_valid = true;
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
        vt: &mut Terminal<'a, '_>,
    ) -> io::Result<()> {
        // Scrollback only exists on the primary screen; the flush boundary
        // pin also lives there, so don't touch it while the alternate screen
        // is active.
        if vt.active_screen().ok() != Some(Screen::Primary) {
            return self.render(stdout, vt);
        }
        let vt_scrollback = vt.scrollback_rows().unwrap_or(0);
        self.normalize_fallback_boundary(vt_scrollback);
        let boundary = self
            .flush_boundary
            .as_ref()
            .and_then(|b| b.point(PointSpace::Screen).ok().flatten());
        let start = boundary
            .map(|p| p.y as usize)
            .or(self.flush_boundary_rows)
            .unwrap_or(0);
        let resize_end = self
            .resize_flush_end_rows
            .or_else(|| {
                self.resize_flush_end
                    .as_ref()
                    .and_then(|b| b.point(PointSpace::Screen).ok().flatten())
                    .map(|p| p.y as usize)
            })
            .map(|y| y.min(vt_scrollback));
        let end = resize_end.unwrap_or(vt_scrollback);
        let mut flush_rows = end.saturating_sub(start.min(end));

        // A tracked point in the middle of a row means the boundary was
        // created by an older, unsafe reflow. Never emit that row wholesale:
        // its prefix may already be in native scrollback. New boundaries are
        // kept at logical-row starts below, so this is only a degraded case.
        let boundary_unsafe = boundary.is_some_and(|p| p.x != 0)
            || (flush_rows > 0
                && vt
                    .grid_ref(screen_point(start as u32))
                    .and_then(|gr| gr.row())
                    .and_then(|row| row.is_wrap_continuation())
                    .unwrap_or(true));
        if boundary_unsafe {
            flush_rows = 0;
        } else {
            // Do not flush a partial soft-wrapped logical line. The native
            // terminal cannot reflow text it has already received, so wait
            // until the candidate prefix ends on a hard row.
            while flush_rows > 0 {
                let point = screen_point((start + flush_rows - 1) as u32);
                let wrapped = vt
                    .grid_ref(point)
                    .and_then(|gr| gr.row())
                    .and_then(|row| row.is_wrapped())
                    .unwrap_or(true);
                if !wrapped {
                    break;
                }
                flush_rows -= 1;
            }
        }

        let mut flushed_total = 0usize;
        let mut incomplete = false;
        if flush_rows > 0 && self.content_rows > 0 {
            let batch_size = self.content_rows as usize;
            queue!(
                stdout,
                SetScrollRegion {
                    top: 1,
                    bottom: self.content_rows
                }
            )?;

            let mut screen_y = start;
            let mut remaining = flush_rows;
            while remaining > 0 {
                let count = remaining.min(batch_size);
                let mut drawn = 0;
                for i in 0..count {
                    queue!(
                        stdout,
                        cursor::MoveTo(0, i as u16),
                        Clear(ClearType::CurrentLine)
                    )?;
                }
                let mut prev_was_wrap_source = false;
                for i in 0..count {
                    let row_point = screen_point(screen_y as u32);
                    let Some(row) = vt.grid_ref(row_point).and_then(|gr| gr.row()).ok() else {
                        incomplete = true;
                        break;
                    };
                    let is_continuation = row.is_wrap_continuation().unwrap_or(true);
                    let is_wrap_source = row.is_wrapped().unwrap_or(true);
                    if !(is_continuation && prev_was_wrap_source) {
                        queue!(stdout, cursor::MoveTo(0, i as u16))?;
                    }
                    self.write_row_content(stdout, vt, row_point)?;
                    screen_y += 1;
                    drawn += 1;
                    prev_was_wrap_source = is_wrap_source;
                }
                if drawn > 0 {
                    queue!(stdout, cursor::MoveTo(0, self.content_rows - 1))?;
                    for _ in 0..drawn {
                        stdout.write_all(b"\n")?;
                    }
                }
                flushed_total += drawn;
                remaining -= count;
                if incomplete || drawn < count {
                    incomplete = true;
                    break;
                }
            }
            queue!(stdout, ResetScrollRegion)?;
        }

        let resize_flush = resize_end.is_some();
        let successful_flush = !boundary_unsafe && !incomplete && flushed_total == flush_rows;
        if successful_flush && (flushed_total > 0 || resize_flush) {
            let has_remaining = !resize_flush && start + flushed_total < end;
            // A resize endpoint excludes rows that the native terminal already
            // received as part of its old viewport. For an ordinary flush,
            // retain a boundary at the first still-pending logical row rather
            // than jumping over a partial soft-wrapped line.
            let anchor = if resize_flush || !has_remaining {
                active_point(0)
            } else {
                screen_point((start + flushed_total) as u32)
            };
            let re_anchored = match &mut self.flush_boundary {
                Some(b) => b.set(vt, anchor).is_ok(),
                None => {
                    self.flush_boundary = vt.track_grid_ref(anchor).ok();
                    self.flush_boundary.is_some()
                }
            };
            if re_anchored {
                self.flush_boundary_rows = None;
            } else {
                // Preserve the emitted prefix when a deferred wrapped suffix
                // remains. Dropping the boundary would make the next retry
                // start at row zero and emit that prefix again. A numeric
                // boundary is safe as a fallback: history pruning is handled
                // by normalize_fallback_boundary before the next write/flush.
                let fallback = if resize_flush {
                    vt_scrollback
                } else {
                    start.saturating_add(flushed_total).min(vt_scrollback)
                };
                tracing::warn!(
                    boundary = fallback,
                    "failed to re-anchor flush boundary pin"
                );
                self.flush_boundary = None;
                self.flush_boundary_rows = Some(fallback);
            }
            if resize_flush {
                // The resize-generated suffix is already represented by the
                // native terminal and must not become pending later.
                self.resize_flush_end = None;
                self.resize_flush_end_rows = None;
            }

            if incomplete || flushed_total >= self.prev_lines.len() {
                self.prev_lines_valid = false;
            } else {
                // Rotate instead of draining so row allocations remain reusable.
                self.prev_lines.rotate_left(flushed_total);
                let invalid_from = self.prev_lines.len() - flushed_total;
                for row in &mut self.prev_lines[invalid_from..] {
                    row.clear();
                }
            }
            self.pending_scroll = self.pending_scroll.max(1);
        }
        self.render(stdout, vt)
    }

    /// Full redraw of all VT lines (after resize or initialization).
    fn render_full(&mut self, stdout: &mut impl Write, vt: &Terminal<'a, '_>) -> io::Result<()> {
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
        self.prev_lines_valid = false;
    }

    /// Snapshot VT state into prev_lines without writing anything to stdout.
    /// Used after TUI handoff to establish a baseline for diff rendering
    /// while preserving existing terminal content.
    fn sync(&mut self, vt: &Terminal<'_, '_>) {
        let rows = vt.rows().unwrap_or(0) as usize;
        let cols = vt.cols().unwrap_or(0) as usize;
        self.prev_lines
            .resize_with(rows, || Vec::with_capacity(cols));
        self.prev_lines.truncate(rows);
        for (y, cells) in self.prev_lines.iter_mut().enumerate() {
            cells_in_row_into(vt, active_point(y as u32), cells);
        }
        self.prev_lines_valid = true;
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
                let mut buf = [0u8; 4096];
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
            let mut renderer = Renderer::new(pty_size.rows, &vt)?;
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
    fn event_loop<'a>(
        &mut self,
        pty: &Arc<Pty>,
        vt: &mut Terminal<'a, '_>,
        renderer: &mut Renderer<'a>,
        event_rx: std::sync::mpsc::Receiver<Event>,
        coordinator_tx: &tokio_mpsc::Sender<ShellEvent>,
        stdout: &mut Box<dyn Write + Send>,
    ) -> Result<Option<u32>, SessionError> {
        let spinner_interval = Duration::from_millis(SPINNER_INTERVAL_MS);
        let mut scanner = EscapeScanner::new();
        let mut vt_input_filter = VtInputFilter::new();
        let mut utf8_acc = Utf8Accumulator::new();
        let mut esc = EscapeState::new();
        let mut resize_pending = false;
        let mut esc_events = Vec::new();
        let mut vt_input = Vec::new();

        loop {
            // Use select! to handle both events and spinner animation
            let event = if resize_pending {
                resize_pending = false;
                Some(Event::Resize)
            } else if self.status_line.state().building {
                match event_rx.recv_timeout(spinner_interval) {
                    Ok(event) => Some(event),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if self.config.show_status_line && !synchronized_output_active(vt) {
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
                        if self.config.show_status_line && !synchronized_output_active(vt) {
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
                            let synchronized = synchronized_output_active(vt);
                            if state.show_error {
                                let error = state.error.clone().unwrap();
                                let mut error_text =
                                    String::from("\r\n\x1b[1;31mBuild error:\x1b[0m\r\n");
                                for line in error.lines() {
                                    error_text.push_str(&format!("  {}\r\n", line));
                                }
                                error_text.push_str("\r\n");
                                renderer.feed(vt, &error_text);
                                if !synchronized {
                                    if renderer.row_offset > 0 {
                                        renderer.render(stdout, vt)?;
                                    } else {
                                        renderer.render_with_scroll(stdout, vt)?;
                                    }
                                }
                            } else {
                                pty.write_all(&[0x0C])?;
                                pty.flush()?;
                            }
                            if !synchronized {
                                self.status_line
                                    .draw(stdout, self.size.cols, self.size.rows)?;
                                renderer.write_cursor(stdout, vt)?;
                                stdout.flush()?;
                            }
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
                    // Preserve deferred erase/scrollback flags across an
                    // application-controlled synchronized-output window.
                    // They are consumed when mode 2026 is released.
                    if !synchronized_output_active(vt) {
                        esc.reset_batch();
                    }
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
                    let filtered = vt_input_filter.filter(&data, &mut vt_input);
                    let text = utf8_acc.accumulate(filtered);
                    let mut total_scroll = renderer.feed(vt, &text);

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
                                let filtered = vt_input_filter.filter(&more, &mut vt_input);
                                let text = utf8_acc.accumulate(filtered);
                                total_scroll += renderer.feed(vt, &text);
                            }
                            Event::PtyExit(exit_code) => {
                                let synchronized = synchronized_output_active(vt);
                                escape_state_cleanup(&esc, stdout)?;
                                if !synchronized {
                                    queue!(stdout, terminal::BeginSynchronizedUpdate)?;
                                }
                                renderer.render_with_scroll(stdout, vt)?;
                                queue!(stdout, terminal::EndSynchronizedUpdate)?;
                                stdout.flush()?;
                                return Ok(exit_code);
                            }
                            Event::Stdin(stdin_data) => {
                                if !&stdin_data.is_empty() {
                                    pty.write_all(&stdin_data)?;
                                    pty.flush()?;
                                }
                            }
                            Event::Command(cmd) => {
                                total_scroll += self.handle_command(cmd, vt, renderer)?;
                            }
                            Event::Resize => {
                                resize_pending = true;
                                break;
                            }
                        }
                    }

                    // Match Ghostty's renderer: do not snapshot or draw an
                    // intermediate terminal state while the application owns
                    // a synchronized-output window. Forwarded control
                    // sequences still need to reach the native terminal.
                    if synchronized_output_active(vt) {
                        stdout.flush()?;
                        continue;
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
                        // `pending_scroll` spans every batch deferred by mode
                        // 2026, whereas `total_scroll` only covers this batch.
                        let need = renderer.pending_scroll.max(total_scroll).max(cursor_excess);

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
                    let synchronized = synchronized_output_active(vt);
                    if synchronized {
                        // Present the final deferred frame before releasing
                        // mode 2026, even if the child exits without doing so.
                        renderer.render_with_scroll(stdout, vt)?;
                    }
                    self.clear_status_row(stdout, esc.in_alternate_screen)?;
                    escape_state_cleanup(&esc, stdout)?;
                    if synchronized {
                        queue!(stdout, terminal::EndSynchronizedUpdate)?;
                    }
                    stdout.flush()?;
                    return Ok(exit_code);
                }

                Event::Command(cmd) => {
                    self.handle_command(cmd, vt, renderer)?;
                    if synchronized_output_active(vt) {
                        continue;
                    }
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
                        // On a primary-screen height reduction, Ghostty moves
                        // the old viewport into its history. The native PTY
                        // has already retained those same rows, so remember
                        // the old active top as an upper flush endpoint before
                        // mutating the VT grid.
                        let old_native_rows = self.size.rows;
                        let shrinking_primary =
                            primary_height_shrunk(old_native_rows, rows, vt.active_screen().ok());
                        self.size = PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        };
                        let resize_end = if shrinking_primary {
                            let old_scrollback = vt.scrollback_rows().unwrap_or(0);
                            (
                                vt.track_grid_ref(active_point(0)).ok(),
                                Some(old_scrollback),
                            )
                        } else {
                            (None, None)
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
                        } else if shrinking_primary {
                            renderer.resize_flush_end = resize_end.0;
                            renderer.resize_flush_end_rows = resize_end.1;
                        }
                        // The resize reflowed VT history. Flush only the old
                        // pending prefix; rows moved out of the old viewport
                        // are already present in native scrollback.
                        renderer.invalidate();
                        if !synchronized_output_active(vt) {
                            renderer.render_with_scroll(stdout, vt)?;
                            if self.config.show_status_line && !esc.in_alternate_screen {
                                self.status_line.draw(stdout, cols, rows)?;
                            }
                            renderer.write_cursor(stdout, vt)?;
                            stdout.flush()?;
                        }
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

        let synchronized = synchronized_output_active(vt);
        if synchronized {
            renderer.render_with_scroll(stdout, vt)?;
        }
        self.clear_status_row(stdout, esc.in_alternate_screen)?;
        escape_state_cleanup(&esc, stdout)?;
        if synchronized {
            queue!(stdout, terminal::EndSynchronizedUpdate)?;
        }
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
        renderer: &mut Renderer<'_>,
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
                return Ok(renderer.feed(vt, &text));
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
        renderer: &Renderer<'_>,
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
    use crate::vt_utils::row_plain_text;
    use portable_pty::CommandBuilder;

    const COLS: u16 = 20;
    const ROWS: u16 = 5;

    fn test_vt_with_size(
        cols: u16,
        rows: u16,
        max_scrollback: usize,
    ) -> Terminal<'static, 'static> {
        Terminal::new(TerminalOptions {
            cols,
            rows,
            max_scrollback,
        })
        .expect("terminal")
    }

    fn test_vt(max_scrollback: usize) -> Terminal<'static, 'static> {
        test_vt_with_size(COLS, ROWS, max_scrollback)
    }

    fn test_renderer<'a>(vt: &Terminal<'a, '_>) -> Renderer<'a> {
        Renderer::new(ROWS, vt).expect("renderer")
    }

    fn test_renderer_with_rows<'a>(rows: u16, vt: &Terminal<'a, '_>) -> Renderer<'a> {
        Renderer::new(rows, vt).expect("renderer")
    }

    /// Replay renderer output into a fresh VT and return all non-empty lines
    /// (scrollback + viewport), trimmed.
    fn replayed_lines(bytes: &[u8]) -> Vec<String> {
        replayed_lines_with_size(bytes, COLS, ROWS)
    }

    fn replayed_lines_with_size(bytes: &[u8], cols: u16, rows: u16) -> Vec<String> {
        let mut check = test_vt_with_size(cols, rows, DEFAULT_MAX_SCROLLBACK);
        check.vt_write(bytes);
        let scrollback = check.scrollback_rows().unwrap_or(0);
        let total = scrollback + rows as usize;
        (0..total)
            .map(|y| {
                row_plain_text(&check, screen_point(y as u32))
                    .trim_end()
                    .to_string()
            })
            .filter(|l| !l.is_empty())
            .collect()
    }

    /// Replay renderer output into a fresh VT and return the active rows.
    fn replayed_viewport(bytes: &[u8]) -> Vec<String> {
        let mut check = test_vt(DEFAULT_MAX_SCROLLBACK);
        check.vt_write(bytes);
        (0..ROWS)
            .map(|y| {
                row_plain_text(&check, active_point(y as u32))
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    fn viewport_lines(vt: &Terminal<'_, '_>) -> Vec<String> {
        (0..ROWS)
            .map(|y| {
                row_plain_text(vt, active_point(y as u32))
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn resize_shrink_uses_native_height_with_status_line() {
        let session = ShellSession::new(SessionConfig {
            show_status_line: true,
            size: Some(PtySize {
                cols: COLS,
                rows: ROWS + 1,
                pixel_width: 0,
                pixel_height: 0,
            }),
        });
        let old_native_rows = session.size.rows;
        let old_content_rows = session.pty_size().rows;
        let new_native_rows = old_native_rows - 1;

        // The one-row native shrink only reaches the PTY's old content height
        // because the status line consumed the other row. The resize endpoint
        // must nevertheless be installed for this primary-screen change.
        assert_eq!(old_content_rows, new_native_rows);
        assert!(primary_height_shrunk(
            old_native_rows,
            new_native_rows,
            Some(Screen::Primary)
        ));
    }

    #[test]
    fn numeric_fallback_boundary_keeps_flushed_prefix_accounted() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        for i in 0..8 {
            renderer.feed(&mut vt, &format!("line{}\r\n", i));
        }
        let scrollback = vt.scrollback_rows().unwrap();
        assert!(scrollback > 2);

        // Model a failed re-anchor after the first two rows were emitted.
        // The remaining suffix must be the only region eligible for retry.
        renderer.flush_boundary = None;
        renderer.flush_boundary_rows = Some(2);
        assert_eq!(renderer.unflushed(&vt), scrollback - 2);

        // The fallback boundary must continue to advance after the suffix is
        // emitted, rather than falling back to row zero.
        renderer.flush_boundary_rows = Some(scrollback);
        assert_eq!(renderer.unflushed(&vt), 0);
    }

    #[test]
    fn renderer_flush_boundary_accounting() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        // Fill the viewport: nothing scrolled off yet.
        for i in 0..ROWS as usize - 1 {
            assert_eq!(renderer.feed(&mut vt, &format!("line{}\r\n", i)), 0);
        }
        assert_eq!(renderer.feed(&mut vt, "line4"), 0);
        assert_eq!(renderer.unflushed(&vt), 0);

        // Each further line scrolls exactly one row into history.
        assert_eq!(renderer.feed(&mut vt, "\r\nline5"), 1);
        assert_eq!(renderer.feed(&mut vt, "\r\nline6"), 1);
        assert_eq!(renderer.unflushed(&vt), 2);

        // A flush consumes the unflushed region and re-anchors the pin.
        out.clear();
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        assert_eq!(renderer.unflushed(&vt), 0);

        // VT history is retained (no CSI 3J wipe), yet not re-flushed.
        assert_eq!(vt.scrollback_rows().unwrap(), 2);
        assert_eq!(renderer.feed(&mut vt, "\r\nline7"), 1);
        assert_eq!(renderer.unflushed(&vt), 1);
    }

    #[test]
    fn renderer_flush_is_incremental() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        for i in 0..8 {
            renderer.feed(&mut vt, &format!("line{}\r\n", i));
        }
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        for i in 8..12 {
            renderer.feed(&mut vt, &format!("line{}\r\n", i));
        }
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();

        // Every line appears exactly once: nothing lost, nothing re-flushed.
        let lines = replayed_lines(&out);
        for i in 0..12 {
            let expected = format!("line{}", i);
            assert_eq!(
                lines.iter().filter(|l| **l == expected).count(),
                1,
                "expected exactly one '{}' in {:?}",
                expected,
                lines
            );
        }
    }

    #[test]
    fn renderer_survives_history_pruning() {
        // Tiny scrollback budget so ghostty prunes history pages between
        // flushes. The old absolute-index accounting desyncs here; the pin
        // must keep the flush sound (no panic, no duplicates).
        let mut vt = test_vt(2_000);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        for i in 0..300 {
            renderer.feed(&mut vt, &format!("line{}\r\n", i));
        }
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        assert_eq!(renderer.unflushed(&vt), 0);

        let lines = replayed_lines(&out);
        // The most recent lines must be present exactly once; older ones may
        // have been pruned before the flush.
        for i in 290..300 {
            let expected = format!("line{}", i);
            assert_eq!(
                lines.iter().filter(|l| **l == expected).count(),
                1,
                "expected exactly one '{}' in flushed output",
                expected
            );
        }

        // Later flushes stay incremental.
        renderer.feed(&mut vt, "after\r\n");
        out.clear();
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        let lines = replayed_lines(&out);
        assert_eq!(lines.iter().filter(|l| **l == "after").count(), 1);
        assert_eq!(lines.iter().filter(|l| **l == "line299").count(), 0);
    }

    #[test]
    fn renderer_second_render_is_empty() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        renderer.feed(&mut vt, "hello\r\nworld");

        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();
        assert!(!out.is_empty());

        // No VT changes: the clean-frame shortcut must emit nothing.
        out.clear();
        renderer.render(&mut out, &vt).unwrap();
        assert!(
            out.is_empty(),
            "clean frame emitted {} bytes: {:?}",
            out.len(),
            String::from_utf8_lossy(&out)
        );
    }

    #[test]
    fn renderer_full_dirty_redraws_unchanged_rows() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        // Switching screens is globally dirty even when both screens contain
        // identical blank cells. Raw-cell diffing must not suppress the full
        // rebuild requested by Ghostty's render state.
        renderer.feed(&mut vt, "\x1b[?1049h");
        out.clear();
        renderer.render(&mut out, &vt).unwrap();

        let clears = out.windows(4).filter(|w| *w == b"\x1b[2K").count();
        assert_eq!(clears, ROWS as usize);
    }

    #[test]
    fn synchronized_output_mode_is_detected() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        assert!(!synchronized_output_active(&vt));

        vt.vt_write(b"\x1b[?2026hintermediate");
        assert!(synchronized_output_active(&vt));

        vt.vt_write(b"\x1b[?2026lfinal");
        assert!(!synchronized_output_active(&vt));
    }

    #[test]
    fn renderer_invalidation_reuses_cell_buffers() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        renderer.feed(&mut vt, "line0\r\nline1\r\nline2\r\nline3\r\nline4");

        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();
        let cell_capacity = renderer.cells_buf.capacity()
            + renderer.prev_lines.iter().map(Vec::capacity).sum::<usize>();

        renderer.invalidate();
        out.clear();
        renderer.render(&mut out, &vt).unwrap();

        let reused_capacity = renderer.cells_buf.capacity()
            + renderer.prev_lines.iter().map(Vec::capacity).sum::<usize>();
        assert_eq!(reused_capacity, cell_capacity);
    }

    #[test]
    fn renderer_redraws_only_changed_row() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        for i in 0..ROWS as usize - 1 {
            renderer.feed(&mut vt, &format!("line{}\r\n", i));
        }
        renderer.feed(&mut vt, "line4");
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        // Overwrite a single character on row 0 (no scroll).
        renderer.feed(&mut vt, "\x1b[1;1HX");
        out.clear();
        renderer.render(&mut out, &vt).unwrap();

        // Only the changed row is cleared and redrawn.
        let clears = out.windows(4).filter(|w| *w == b"\x1b[2K").count();
        assert_eq!(
            clears,
            1,
            "expected 1 row redraw, got {}: {:?}",
            clears,
            String::from_utf8_lossy(&out)
        );
        assert_eq!(replayed_viewport(&out)[0], "Xine0");
    }

    #[test]
    fn renderer_preserves_graphemes_and_extended_styles() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        renderer.feed(&mut vt, "\x1b[4:3;58;2;1;2;3;8;9;53me\u{301}\x1b[0m");

        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        let mut replay = test_vt(DEFAULT_MAX_SCROLLBACK);
        replay.vt_write(&out);
        assert_eq!(
            row_plain_text(&replay, active_point(0)).trim_end(),
            "e\u{301}"
        );

        let original_style = vt.grid_ref(active_point(0)).unwrap().style().unwrap();
        let replayed_style = replay.grid_ref(active_point(0)).unwrap().style().unwrap();
        assert_eq!(replayed_style, original_style);
        assert!(replayed_style.invisible);
        assert!(replayed_style.strikethrough);
        assert!(replayed_style.overline);
        assert_eq!(replayed_style.underline, Underline::Curly);
    }

    #[test]
    fn renderer_region_scroll_stays_in_sync() {
        // A DECSTBM region scroll moves rows without creating scrollback.
        // The dirty-tracking fast path must still pick up every moved row.
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        for i in 0..ROWS as usize {
            renderer.feed(&mut vt, &format!("line{}\r\n", i));
        }
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        // Scroll rows 2-4 up by one inside a region, then reset the region.
        assert_eq!(renderer.feed(&mut vt, "\x1b[2;4r\x1b[4;1H\nnew\x1b[r"), 0);
        renderer.render(&mut out, &vt).unwrap();

        assert_eq!(replayed_viewport(&out), viewport_lines(&vt));
    }

    #[test]
    fn renderer_height_shrink_without_pending_history_does_not_reflush() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.feed(&mut vt, "line0\r\nline1\r\nline2\r\nline3\r\nold-bottom");
        renderer.render_full(&mut out, &vt).unwrap();
        out.clear();
        assert_eq!(renderer.unflushed(&vt), 0);

        let old_scrollback = vt.scrollback_rows().unwrap();
        let end = vt.track_grid_ref(active_point(0)).unwrap();
        vt.resize(COLS, ROWS - 1, 0, 0).unwrap();
        renderer.content_rows = ROWS - 1;
        renderer.resize_flush_end = Some(end);
        renderer.resize_flush_end_rows = Some(old_scrollback);
        renderer.invalidate();
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();

        let lines = replayed_lines_with_size(&out, COLS, ROWS - 1);
        assert_eq!(
            lines.iter().filter(|line| line.as_str() == "line0").count(),
            0,
            "resize output unexpectedly re-emitted dropped row: {lines:?}"
        );
    }

    #[test]
    fn renderer_resize_does_not_reemit_soft_wrap_prefix() {
        const OLD_COLS: u16 = 10;
        const NEW_COLS: u16 = 16;
        const TEST_ROWS: u16 = 5;

        let mut vt = test_vt_with_size(OLD_COLS, TEST_ROWS, DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer_with_rows(TEST_ROWS, &vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        renderer.feed(
            &mut vt,
            "abcdefghijABCDEFGHIJ\r\nline2\r\nline3\r\nline4\r\n",
        );
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();

        vt.resize(NEW_COLS, TEST_ROWS, 0, 0).unwrap();
        renderer.invalidate();
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        renderer.feed(&mut vt, "\r\nline5\r\nline6\r\nline7\r\nline8\r\n");
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();

        let text = replayed_lines_with_size(&out, NEW_COLS, TEST_ROWS).join("\n");
        assert_eq!(
            text.matches("abcdefghij").count(),
            1,
            "re-emitted prefix: {text:?}"
        );
        assert_eq!(
            text.matches("ABCDEF").count(),
            1,
            "lost continuation prefix: {text:?}"
        );
        assert_eq!(
            text.matches("GHIJ").count(),
            1,
            "lost continuation suffix: {text:?}"
        );
    }

    #[test]
    fn renderer_resize_preserves_unflushed_scrollback() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        for i in 0..8 {
            renderer.feed(&mut vt, &format!("line{}\r\n", i));
        }
        assert!(renderer.unflushed(&vt) > 0);

        // Resize with unflushed rows pending: history reflows, the pin
        // follows, and the flush after resize emits the pending rows
        // instead of discarding them. Capture the old viewport endpoint just
        // as the session resize path does.
        let old_scrollback = vt.scrollback_rows().unwrap();
        let resize_end = vt.track_grid_ref(active_point(0)).unwrap();
        vt.resize(COLS, ROWS - 1, 0, 0).unwrap();
        renderer.content_rows = ROWS - 1;
        renderer.resize_flush_end = Some(resize_end);
        renderer.resize_flush_end_rows = Some(old_scrollback);
        renderer.invalidate();
        out.clear();
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();

        let lines = replayed_lines_with_size(&out, COLS, ROWS - 1);
        for i in 0..4 {
            let expected = format!("line{}", i);
            assert_eq!(
                lines.iter().filter(|line| **line == expected).count(),
                1,
                "expected exactly one '{}' after resize, got {:?}",
                expected,
                lines
            );
        }
        assert_eq!(
            lines.iter().filter(|line| **line == "line4").count(),
            0,
            "old viewport row was re-flushed after resize: {lines:?}"
        );
    }

    fn filter_chunks(chunks: &[&[u8]]) -> Vec<u8> {
        let mut filter = VtInputFilter::new();
        let mut output = Vec::new();
        let mut combined = Vec::new();

        for chunk in chunks {
            let filtered = filter.filter(chunk, &mut output);
            combined.extend_from_slice(filtered);
        }

        combined
    }

    #[test]
    fn vt_input_filter_strips_tmux_title_sequence() {
        let filtered = filter_chunks(&[b"hello \x1bkecho hello\x1b\\world"]);
        assert_eq!(filtered, b"hello world");
    }

    #[test]
    fn vt_input_filter_strips_tmux_title_sequence_across_chunks() {
        let filtered = filter_chunks(&[b"hello \x1bkec", b"ho hello", b"\x1b\\world"]);
        assert_eq!(filtered, b"hello world");
    }

    #[test]
    fn vt_input_filter_preserves_other_escape_sequences() {
        let filtered = filter_chunks(&[b"hello \x1b[31mred\x1b[0m"]);
        assert_eq!(filtered, b"hello \x1b[31mred\x1b[0m");
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
