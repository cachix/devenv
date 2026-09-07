//! Shell session management.
//!
//! This module provides the main `ShellSession` type that orchestrates
//! PTY lifecycle, terminal I/O, and status line rendering.

use crate::escape::EscapeScanner;
use crate::escape_state::{
    EscapeState, cleanup_forwarded_modes as escape_state_cleanup,
    process_escape_events as escape_state_process,
};
use crate::keybindings::{ShellAction, ShellKeybindings};
use crate::protocol::{ShellCommand, ShellEvent};
use crate::pty::{Pty, PtyError, get_terminal_size};
use crate::status_line::{SPINNER_INTERVAL_MS, StatusLine};
use crate::terminal::RawModeGuard;
use crate::terminal_commands::{
    InBandResizeNotification, ORIGIN_MODE, ResetDecMode, ResetScrollRegion, SetScrollRegion,
};
use crate::vt_utils::{
    CursorState, DEFAULT_MAX_SCROLLBACK, active_point, point_with_x, screen_point,
};
use crossterm::{
    Command, cursor, queue,
    terminal::{self, Clear, ClearType},
};
use devenv_mailbox::{FrontendCommand, FrontendEvent};
use libghostty_vt::fmt::{Format, Formatter, FormatterOptions};
use libghostty_vt::render::{
    CellIteration, CellIterator, Colors, Dirty, RenderState, RowIteration, RowIterator,
};
use libghostty_vt::screen::{CellContentTag, Screen, TrackedGridRef};
use libghostty_vt::selection::Selection;
use libghostty_vt::style::{PaletteIndex, RgbColor};
#[cfg(test)]
use libghostty_vt::style::{StyleColor, Underline};
use libghostty_vt::terminal::{Mode, Point, PointCoordinate, PointSpace, Terminal};
use portable_pty::PtySize;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{self, IsTerminal, Read, Write};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc as tokio_mpsc;
use tokio_util::sync::CancellationToken;

/// PTY reads are deliberately bounded so a busy child cannot turn the event
/// channel into an unbounded allocation queue. The reader blocks once all of
/// these buffers are pending; the kernel PTY buffer then provides ordinary
/// backpressure to the child without dropping or coalescing bytes.
///
/// Each buffer is sized to hold a full-screen repaint burst in one read where
/// possible, reducing syscalls, event allocations, and fragmented renderer
/// frames. Platforms whose PTY returns less per read simply fill less of it.
///
/// The count only needs to keep the reader off the kernel PTY buffer while the
/// renderer works, so it stays low: `COUNT * BYTES` is committed up front and
/// bounds how much unrendered output can queue up.
const PTY_READ_BUFFER_BYTES: usize = 64 * 1024;
const PTY_READ_BUFFER_COUNT: usize = 4;

/// A render pass consumes at most this much output (or this many events).
/// This bounds the time spent parsing output before we present a frame and
/// gives pending stdin, resize, and coordinator events a regular chance to
/// run. A single PTY read may make a batch one read larger than the byte cap.
const MAX_PTY_BATCH_BYTES: usize = 32 * 1024;
const MAX_RENDER_BATCH_EVENTS: usize = 16;

#[derive(Clone, Copy)]
enum FramePlan {
    Skip,
    DirtyRows,
    CompareRows,
    Redraw,
}

#[derive(Clone, Copy)]
enum RowPlan {
    /// Encode immediately. The entire frame must be rebuilt.
    Redraw,
    /// Format a canonical VT row, then compare it with the host baseline.
    Compare,
}

impl FramePlan {
    fn for_snapshot(
        dirty: Dirty,
        cache_valid: bool,
        viewport_shifted: bool,
        primary_screen: bool,
        globals_changed: bool,
    ) -> Self {
        if !cache_valid {
            return Self::Redraw;
        }
        if dirty == Dirty::Full {
            return if viewport_shifted && !globals_changed {
                Self::CompareRows
            } else {
                Self::Redraw
            };
        }
        if viewport_shifted || !primary_screen {
            return Self::CompareRows;
        }
        match dirty {
            Dirty::Clean => Self::Skip,
            Dirty::Partial => Self::DirtyRows,
            Dirty::Full => unreachable!(),
        }
    }

    fn row(self, dirty: bool) -> Option<RowPlan> {
        match self {
            Self::Skip => None,
            Self::DirtyRows if dirty => Some(RowPlan::Compare),
            Self::DirtyRows => None,
            Self::CompareRows => Some(RowPlan::Compare),
            Self::Redraw => Some(RowPlan::Redraw),
        }
    }
}

struct FrameCache {
    // These are Ghostty's canonical VT bytes for each row, not raw cells.
    // Comparing the presentation form makes viewport-shift cache reuse safe
    // when a row's resolved style or formatter behavior changes.
    rows: Vec<Vec<u8>>,
    valid: bool,
}

impl FrameCache {
    fn new(rows: usize, encoded_row_capacity: usize) -> Self {
        Self {
            rows: (0..rows)
                .map(|_| Vec::with_capacity(encoded_row_capacity))
                .collect(),
            valid: false,
        }
    }

    fn covers(&self, visible_rows: usize) -> bool {
        self.valid && self.rows.len() >= visible_rows
    }

    fn matches(&self, row_idx: usize, bytes: &[u8]) -> bool {
        self.valid && self.rows.get(row_idx).is_some_and(|row| row == bytes)
    }

    fn store(&mut self, row_idx: usize, bytes: &mut Vec<u8>) {
        if row_idx >= self.rows.len() {
            let capacity = bytes.capacity();
            self.rows
                .resize_with(row_idx + 1, || Vec::with_capacity(capacity));
        }
        // Keep the formatter's high-water buffer. Swapping it with a cache
        // slot would make the next row start from that slot's (often much
        // smaller) capacity and repeatedly grow on styled frames.
        let row = &mut self.rows[row_idx];
        row.clear();
        row.extend_from_slice(bytes);
    }

    fn invalidate(&mut self) {
        self.valid = false;
    }

    fn shift_left(&mut self, count: usize) {
        if count >= self.rows.len() {
            self.invalidate();
            return;
        }
        self.rows.rotate_left(count);
        let invalid_from = self.rows.len() - count;
        for row in &mut self.rows[invalid_from..] {
            row.clear();
        }
    }
}

/// Formats one complete terminal row through Ghostty's native VT formatter.
///
/// `RenderState` remains responsible for deciding *which* rows need work.
/// Formatting the selected row in Ghostty avoids a Rust/C ABI crossing for
/// every field of every cell and keeps its grapheme and style rules in one
/// place.
struct RowFormatter {
    bytes: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CellBackground {
    Palette(PaletteIndex),
    Rgb(RgbColor),
}

impl RowFormatter {
    fn new(cols: usize) -> Self {
        Self {
            // VT output can be substantially larger than text for styled
            // rows. Start with a useful retained buffer and grow only when
            // Ghostty tells us the exact required size.
            bytes: vec![0; cols.saturating_mul(16).max(256)],
        }
    }

    fn format_row(
        &mut self,
        vt: &Terminal<'_, '_>,
        point: Point,
    ) -> Result<&[u8], libghostty_vt::Error> {
        // `Formatter::format_buf` takes a slice, so restore the retained
        // vector's full capacity after the prior row was truncated to its
        // written length. This avoids a needless OutOfSpace/retry for a
        // longer row following a short one.
        self.bytes.resize(self.bytes.capacity(), 0);
        let cols = vt.cols()?;
        if cols == 0 {
            self.bytes.clear();
            return Ok(&self.bytes);
        }

        let start = vt.grid_ref(point)?;
        let end = vt.grid_ref(point_with_x(point, cols - 1))?;
        let selection = Selection::new(start, end, true);
        // Unlike Terminal::format_selection_buf, the configurable formatter
        // leaves all terminal/screen extras disabled by default. A row repaint
        // must contain cell presentation only: palette, mode, cursor, and
        // other global state are handled at the mux boundary.
        let options = FormatterOptions::new()
            .with_format(Format::Vt)
            // Each row is positioned and cleared independently, matching
            // the old encoder's omission of trailing default blanks. Ghostty
            // also omits rows containing only background cells; the repaint
            // path restores those below.
            .with_unwrap(false)
            .with_trim(true)
            .with_selection(&selection);
        let mut formatter = Formatter::new(vt, options)?;
        loop {
            match formatter.format_buf(&mut self.bytes) {
                Ok(written) => {
                    self.bytes.truncate(written);
                    return Ok(&self.bytes);
                }
                Err(libghostty_vt::Error::OutOfSpace { .. }) => {
                    // The low-level formatter's buffer API currently loses
                    // the required length in its Rust error conversion. Query
                    // it explicitly only on this cold growth path.
                    let required = formatter.format_len()?;
                    self.bytes.resize(required.max(self.bytes.len() * 2), 0);
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn restore_snapshot_backgrounds(
        &mut self,
        cells: &mut CellIteration<'_, '_>,
    ) -> Result<(), libghostty_vt::Error> {
        // Formatter output is optimized for serialization and always trims a
        // line with no text, even when EL/ECH left visible background-only
        // cells. A terminal repaint cannot drop those cells. Use Ghostty's
        // resolved render colors only for this otherwise-empty row.
        self.restore_backgrounds(|| {
            cells
                .next()
                .map(|cell| cell.bg_color().map(|color| color.map(CellBackground::Rgb)))
        })
    }

    fn restore_terminal_backgrounds(
        &mut self,
        vt: &Terminal<'_, '_>,
        point: Point,
    ) -> Result<(), libghostty_vt::Error> {
        let mut col = 0;
        let cols = vt.cols()?;
        self.restore_backgrounds(|| {
            if col >= cols {
                return None;
            }
            let result = vt
                .grid_ref(point_with_x(point, col))
                .and_then(|cell_ref| cell_ref.cell())
                .and_then(|cell| {
                    let background = match cell.content_tag()? {
                        CellContentTag::BgColorPalette => {
                            Some(CellBackground::Palette(cell.bg_color_palette()?))
                        }
                        CellContentTag::BgColorRgb => {
                            Some(CellBackground::Rgb(cell.bg_color_rgb()?))
                        }
                        CellContentTag::Codepoint | CellContentTag::CodepointGrapheme => None,
                    };
                    Ok(background)
                });
            col += 1;
            Some(result)
        })
    }

    fn restore_backgrounds(
        &mut self,
        mut next: impl FnMut() -> Option<Result<Option<CellBackground>, libghostty_vt::Error>>,
    ) -> Result<(), libghostty_vt::Error> {
        debug_assert!(self.bytes.is_empty());
        let mut active = None;
        let mut has_background = false;

        while let Some(background) = next() {
            let background = background?;
            has_background |= background.is_some();
            if background != active {
                match background {
                    Some(CellBackground::Palette(color)) => {
                        write!(&mut self.bytes, "\x1b[48;5;{}m", color.0)
                            .expect("writing to Vec cannot fail");
                    }
                    Some(CellBackground::Rgb(color)) => {
                        write!(
                            &mut self.bytes,
                            "\x1b[48;2;{};{};{}m",
                            color.r, color.g, color.b
                        )
                        .expect("writing to Vec cannot fail");
                    }
                    None => self.bytes.extend_from_slice(b"\x1b[49m"),
                }
                active = background;
            }
            self.bytes.push(b' ');
        }

        // An ordinary default blank row still relies on Clear(CurrentLine).
        // Discard the speculative spaces so it retains the formatter's compact
        // representation and does not become a hot-path write amplification.
        if !has_background {
            self.bytes.clear();
        }
        Ok(())
    }
}

enum RowRender {
    Drawn,
    Unchanged,
    Unavailable,
}

struct FrameRenderer {
    cache: FrameCache,
    formatter: RowFormatter,
}

impl FrameRenderer {
    fn new(rows: usize, cols: usize) -> Result<Self, libghostty_vt::Error> {
        Ok(Self {
            cache: FrameCache::new(rows, cols.saturating_mul(16).max(256)),
            formatter: RowFormatter::new(cols),
        })
    }

    fn render_snapshot_row(
        &mut self,
        stdout: &mut impl Write,
        row_idx: usize,
        screen_row: u16,
        plan: RowPlan,
    ) -> io::Result<RowRender> {
        if !matches!(plan, RowPlan::Redraw) && self.cache.matches(row_idx, &self.formatter.bytes) {
            return Ok(RowRender::Unchanged);
        }

        queue!(
            stdout,
            cursor::MoveTo(0, screen_row),
            Clear(ClearType::CurrentLine)
        )?;
        stdout.write_all(&self.formatter.bytes)?;
        // The selection formatter omits a reset for a default/blank row.
        // A row draw must nevertheless leave the physical terminal neutral
        // because Clear does not alter SGR state.
        stdout.write_all(b"\x1b[0m")?;
        self.cache.store(row_idx, &mut self.formatter.bytes);
        Ok(RowRender::Drawn)
    }

    fn render_terminal_row(
        &mut self,
        stdout: &mut impl Write,
        vt: &Terminal<'_, '_>,
        row_idx: usize,
        screen_row: u16,
        compare: bool,
    ) -> io::Result<bool> {
        if self
            .format_terminal_row(vt, active_point(row_idx as u32))
            .is_err()
        {
            return Ok(false);
        }
        if compare && self.cache.matches(row_idx, &self.formatter.bytes) {
            return Ok(false);
        }
        queue!(
            stdout,
            cursor::MoveTo(0, screen_row),
            Clear(ClearType::CurrentLine)
        )?;
        stdout.write_all(&self.formatter.bytes)?;
        stdout.write_all(b"\x1b[0m")?;
        self.cache.store(row_idx, &mut self.formatter.bytes);
        Ok(true)
    }

    fn write_terminal_row(
        &mut self,
        stdout: &mut impl Write,
        vt: &Terminal<'_, '_>,
        point: Point,
    ) -> io::Result<()> {
        if self.format_terminal_row(vt, point).is_ok() {
            stdout.write_all(&self.formatter.bytes)?;
            stdout.write_all(b"\x1b[0m")?;
        }
        Ok(())
    }

    fn format_snapshot_row<'alloc>(
        &mut self,
        vt: &Terminal<'_, '_>,
        row: &RowIteration<'alloc, '_>,
        cells: &mut CellIterator<'alloc>,
        row_idx: usize,
    ) -> Result<(), libghostty_vt::Error> {
        self.formatter
            .format_row(vt, active_point(row_idx as u32))?;
        if self.formatter.bytes.is_empty() {
            let mut cells = cells.update(row)?;
            self.formatter.restore_snapshot_backgrounds(&mut cells)?;
        }
        Ok(())
    }

    fn format_terminal_row(
        &mut self,
        vt: &Terminal<'_, '_>,
        point: Point,
    ) -> Result<(), libghostty_vt::Error> {
        self.formatter.format_row(vt, point)?;
        if self.formatter.bytes.is_empty() {
            self.formatter.restore_terminal_backgrounds(vt, point)?;
        }
        Ok(())
    }

    fn sync(&mut self, vt: &Terminal<'_, '_>) {
        let rows = vt.rows().unwrap_or(0) as usize;
        let capacity = self.formatter.bytes.capacity();
        self.cache
            .rows
            .resize_with(rows, || Vec::with_capacity(capacity));
        self.cache.rows.truncate(rows);
        for row_idx in 0..rows {
            if self
                .format_terminal_row(vt, active_point(row_idx as u32))
                .is_err()
            {
                self.cache.invalidate();
                return;
            }
            self.cache.store(row_idx, &mut self.formatter.bytes);
        }
        self.cache.valid = true;
    }

    #[cfg(test)]
    fn retained_capacities(&self) -> (usize, usize, usize) {
        (
            self.formatter.bytes.capacity()
                + self.cache.rows.iter().map(Vec::capacity).sum::<usize>(),
            0,
            self.formatter.bytes.capacity(),
        )
    }
}

#[derive(Clone, PartialEq, Eq)]
struct FrameGlobals {
    screen: Screen,
    rows: u16,
    cols: u16,
    colors: Colors,
}

struct ViewportRenderer<'a> {
    state: RenderState<'a>,
    rows: RowIterator<'a>,
    cells: CellIterator<'a>,
    frame: FrameRenderer,
    globals: Option<FrameGlobals>,
}

impl<'a> ViewportRenderer<'a> {
    fn new(rows: usize, cols: usize) -> Result<Self, libghostty_vt::Error> {
        Ok(Self {
            state: RenderState::new()?,
            rows: RowIterator::new()?,
            cells: CellIterator::new()?,
            frame: FrameRenderer::new(rows, cols)?,
            globals: None,
        })
    }

    fn render(
        &mut self,
        stdout: &mut impl Write,
        vt: &Terminal<'a, '_>,
        visible_rows: usize,
        row_offset: u16,
        viewport_shifted: bool,
    ) -> io::Result<()> {
        let cache_valid = self.frame.cache.covers(visible_rows);
        let primary_screen = vt.active_screen().ok() == Some(Screen::Primary);
        let mut completed_rows = 0usize;

        match self.state.update(vt) {
            Ok(snapshot) => {
                let dirty = snapshot.dirty().unwrap_or(Dirty::Full);
                // A viewport move makes Ghostty report a full frame even when
                // the native scroll operation already shifted identical rows.
                // Compare rows in that case, but preserve Ghostty's full-redraw
                // contract when screen dimensions, palette, or default colors
                // changed at the same time.
                let refresh_globals = dirty == Dirty::Full || self.globals.is_none();
                let next_globals = if refresh_globals {
                    let values = (snapshot.rows(), snapshot.cols(), snapshot.colors());
                    match values {
                        (Ok(rows), Ok(cols), Ok(colors)) => Some(FrameGlobals {
                            screen: vt.active_screen().unwrap_or(Screen::Primary),
                            rows,
                            cols,
                            colors,
                        }),
                        _ => None,
                    }
                } else {
                    None
                };
                let globals_changed = if refresh_globals {
                    next_globals
                        .as_ref()
                        .is_none_or(|globals| self.globals.as_ref() != Some(globals))
                } else {
                    false
                };
                let plan = FramePlan::for_snapshot(
                    dirty,
                    cache_valid,
                    viewport_shifted,
                    primary_screen,
                    globals_changed,
                );
                if matches!(plan, FramePlan::Skip) {
                    completed_rows = visible_rows;
                } else if let Ok(mut rows) = self.rows.update(&snapshot) {
                    while let Some(row) = rows.next() {
                        let row_idx = completed_rows;
                        if row_idx < visible_rows {
                            let row_dirty = row.dirty().unwrap_or(true);
                            if let Some(row_plan) = plan.row(row_dirty) {
                                let result = if self
                                    .frame
                                    .format_snapshot_row(vt, row, &mut self.cells, row_idx)
                                    .is_ok()
                                {
                                    self.frame.render_snapshot_row(
                                        stdout,
                                        row_idx,
                                        row_idx as u16 + row_offset,
                                        row_plan,
                                    )?
                                } else {
                                    RowRender::Unavailable
                                };
                                if matches!(result, RowRender::Unavailable) {
                                    self.frame.render_terminal_row(
                                        stdout,
                                        vt,
                                        row_idx,
                                        row_idx as u16 + row_offset,
                                        !matches!(plan, FramePlan::Redraw),
                                    )?;
                                }
                            }
                        }
                        let _ = row.set_dirty(false);
                        completed_rows += 1;
                    }
                }
                let _ = snapshot.set_dirty(Dirty::Clean);
                if refresh_globals {
                    self.globals = next_globals;
                }
            }
            Err(error) => {
                tracing::debug!(%error, "render state update failed");
            }
        }

        if completed_rows < visible_rows {
            for row_idx in completed_rows..visible_rows {
                self.frame.render_terminal_row(
                    stdout,
                    vt,
                    row_idx,
                    row_idx as u16 + row_offset,
                    cache_valid,
                )?;
            }
        }
        self.frame.cache.valid = true;
        Ok(())
    }

    fn write_terminal_row(
        &mut self,
        stdout: &mut impl Write,
        vt: &Terminal<'_, '_>,
        point: Point,
    ) -> io::Result<()> {
        self.frame.write_terminal_row(stdout, vt, point)
    }

    fn invalidate(&mut self) {
        self.frame.cache.invalidate();
    }

    fn shift_cache(&mut self, rows: usize) {
        self.frame.cache.shift_left(rows);
    }

    fn sync(&mut self, vt: &Terminal<'_, '_>) {
        self.frame.sync(vt);
    }

    #[cfg(test)]
    fn retained_capacities(&self) -> (usize, usize, usize) {
        self.frame.retained_capacities()
    }
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

/// Bytes that may be sent directly to Ghostty's persistent VT stream parser.
///
/// Ordinary PTY output never needs rewriting. Keeping that common path
/// borrowed avoids copying every read merely to check for a tmux title
/// sequence. The filtered variant borrows the caller's retained scratch
/// buffer only when an escape sequence needs inspection or removal.
enum FilteredVtInput<'a> {
    Borrowed(&'a [u8]),
    Filtered(&'a [u8]),
}

impl FilteredVtInput<'_> {
    fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Borrowed(bytes) | Self::Filtered(bytes) => bytes,
        }
    }
}

impl VtInputFilter {
    fn new() -> Self {
        Self {
            state: VtInputFilterState::Ground,
            pending: Vec::new(),
        }
    }

    fn filter<'a>(&mut self, data: &'a [u8], output: &'a mut Vec<u8>) -> FilteredVtInput<'a> {
        // The vast majority of PTY reads are ordinary output. Do not touch
        // the scratch buffer or copy these bytes: Ghostty owns a persistent
        // byte-stream parser and correctly preserves split UTF-8 sequences.
        if matches!(self.state, VtInputFilterState::Ground) && !data.contains(&0x1b) {
            return FilteredVtInput::Borrowed(data);
        }

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

        FilteredVtInput::Filtered(output)
    }
}

/// Replies emitted synchronously by libghostty-vt while it processes raw PTY
/// output. The callback is deliberately a deny-by-default capability: it only
/// accepts responses that we know must describe the virtual terminal.
///
/// Installing `on_pty_write` enables libghostty-vt responses for several
/// protocols (OSC colors, DA, XTVERSION, DECRQM, resize reports, …). Most of
/// those remain coupled to the physical terminal in this mux, so allowing the
/// callback to write them wholesale would duplicate or contradict the mature
/// stdout passthrough path. See `VirtualTerminalQuery` in `escape` for the
/// two queries we intentionally take ownership of.
#[derive(Default)]
struct VirtualPtyReplies {
    bytes: Vec<u8>,
}

impl VirtualPtyReplies {
    fn capture(&mut self, data: &[u8]) {
        if is_virtual_terminal_reply(data) {
            self.bytes.extend_from_slice(data);
        }
    }

    fn flush_to_pty(&mut self, pty: &Pty) -> io::Result<()> {
        if self.bytes.is_empty() {
            return Ok(());
        }
        pty.write_all(&self.bytes)?;
        pty.flush()?;
        self.bytes.clear();
        Ok(())
    }
}

/// Whether a libghostty-vt `on_pty_write` callback payload can only be the
/// answer to one of the virtual-owned queries. Keep this intentionally exact:
/// accepting a merely similar CSI response would make an unrelated physical
/// query leak back to the child PTY.
fn is_virtual_terminal_reply(data: &[u8]) -> bool {
    data == b"\x1b[0n" || is_cpr_reply(data) || is_in_band_resize_mode_report(data)
}

fn is_cpr_reply(data: &[u8]) -> bool {
    let Some(body) = data
        .strip_prefix(b"\x1b[")
        .and_then(|body| body.strip_suffix(b"R"))
    else {
        return false;
    };
    let Some(separator) = body.iter().position(|&byte| byte == b';') else {
        return false;
    };
    let (row, col_with_separator) = body.split_at(separator);
    let col = &col_with_separator[1..];
    is_ascii_decimal(row) && is_ascii_decimal(col)
}

fn is_in_band_resize_mode_report(data: &[u8]) -> bool {
    let Some(state) = data
        .strip_prefix(b"\x1b[?2048;")
        .and_then(|body| body.strip_suffix(b"$y"))
    else {
        return false;
    };
    matches!(state, b"0" | b"1" | b"2" | b"3" | b"4")
}

fn is_ascii_decimal(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit)
}

fn flush_virtual_pty_replies(replies: &RefCell<VirtualPtyReplies>, pty: &Pty) -> io::Result<()> {
    replies.borrow_mut().flush_to_pty(pty)
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

/// Start a renderer-owned transaction unless the application has already
/// opened mode 2026 and left its reset pending for this frame.
fn begin_renderer_transaction(stdout: &mut impl Write, deferred_release: bool) -> io::Result<()> {
    if !deferred_release {
        queue!(stdout, terminal::BeginSynchronizedUpdate)?;
    }
    Ok(())
}

/// End a renderer transaction. A pending application reset is the boundary
/// when present: writing an additional Begin/End pair inside it can expose the
/// final virtual frame before its status line and cursor are complete.
fn finish_renderer_transaction(
    stdout: &mut impl Write,
    esc: &mut EscapeState,
    transaction_open: bool,
) -> io::Result<()> {
    if !transaction_open {
        return Ok(());
    }
    if !esc.flush_deferred_synchronized_output_reset(stdout)? {
        queue!(stdout, terminal::EndSynchronizedUpdate)?;
    }
    Ok(())
}

struct Renderer<'a> {
    /// Persistent Ghostty snapshot, row iterators, and native-terminal frame
    /// cache. Kept separate from scrollback translation below.
    viewport: ViewportRenderer<'a>,
    /// Previous cursor state.
    prev_cursor: CursorState,
    /// Rows of pre-existing terminal content the session started below.
    /// While > 0, VT row N maps to real terminal row (N + 1 + row_offset);
    /// `make_room` scrolls that content into native scrollback as the shell
    /// needs the space. Never read or written outside this impl.
    row_offset: u16,
    /// Number of usable content rows on the real terminal (excludes status line).
    /// Used to clip rendering so offset VT rows don't overwrite the status line.
    content_rows: u16,
    /// Tracked grid ref (ghostty pin) at the first VT row not yet flushed to
    /// native terminal scrollback. Ghostty keeps it anchored to that row
    /// across scrolling, history pruning, and resize reflow, so flush
    /// accounting stays exact without wiping VT history.
    /// Ghostty marks this pin as garbage only when a reset discards the whole
    /// page list or max-scrollback pruning discards the oldest page. In both
    /// cases the first surviving row is the exact replacement boundary.
    flush_boundary: TrackedGridRef,
    /// Intervals of scrollback rows that primary-screen height shrinks moved
    /// out of the old viewport, oldest first. The native terminal already
    /// shows those rows, so history flushes skip exactly these intervals.
    resize_exclusions: VecDeque<ResizeExclusion>,
    /// Viewport lines scrolled off since the last render; tells `render` that
    /// row content shifted so per-row dirty flags alone can't be trusted.
    pending_scroll: usize,
}

/// Screen rows `[start, end)` that a primary-screen height shrink moved from
/// the old viewport into VT history. The native terminal retained the same
/// rows itself when it shrank, so the history flush must skip exactly this
/// interval: emitting it would duplicate rows, and skipping past `end` would
/// drop rows fed after the resize (those land below `end` and stay pending).
/// Both endpoints are tracked pins, so the interval stays exact across
/// reflow and pruning.
struct ResizeExclusion {
    /// First excluded row: the old viewport top, captured before the resize
    /// mutated the grid.
    start: TrackedGridRef,
    /// First row after the interval: the new viewport top, captured right
    /// after the resize.
    end: TrackedGridRef,
}

impl<'a> Renderer<'a> {
    fn new(content_rows: u16, vt: &Terminal<'a, '_>) -> Result<Self, libghostty_vt::Error> {
        let flush_boundary = vt.track_grid_ref(active_point(0))?;
        let cols = vt.cols().unwrap_or(0) as usize;
        let rows = vt.rows().unwrap_or(0) as usize;
        Ok(Self {
            viewport: ViewportRenderer::new(rows, cols)?,
            prev_cursor: CursorState {
                col: 0,
                row: 0,
                visible: true,
            },
            row_offset: 0,
            content_rows,
            flush_boundary,
            resize_exclusions: VecDeque::new(),
            pending_scroll: 0,
        })
    }

    /// Feed raw VT bytes into Ghostty and return the scroll count (lines that
    /// scrolled off the viewport), measured as growth of the unflushed region.
    fn feed(&mut self, vt: &mut Terminal<'_, '_>, data: impl AsRef<[u8]>) -> io::Result<usize> {
        let data = data.as_ref();
        let before = self.unflushed(vt)?;
        vt.vt_write(data);
        self.reconcile_after_mutation(vt)?;
        let scrolled = self.unflushed(vt)?.saturating_sub(before);
        self.pending_scroll += scrolled;
        Ok(scrolled)
    }

    /// Reconcile tracked refs after `vt_write` or resize. At Ghostty's pinned
    /// revision, tracked refs lose their value in exactly two PageList paths:
    /// reset and oldest-page pruning. Both make screen row zero the first row
    /// whose prior accounting can still matter.
    fn reconcile_after_mutation(&mut self, vt: &mut Terminal<'_, '_>) -> io::Result<()> {
        // Primary pins must not be re-anchored while the alternate page
        // list is active: `set` always targets the active screen. Once input
        // returns to the primary screen, the next reconciliation repairs it.
        if vt.active_screen().ok() != Some(Screen::Primary) {
            return Ok(());
        }
        let boundary_lost = !self.flush_boundary.has_value();
        if boundary_lost {
            self.flush_boundary
                .set(vt, screen_point(0))
                .map_err(io::Error::other)?;
        }
        self.reconcile_exclusions(vt, boundary_lost)
    }

    /// Reconcile resize exclusions after a mutation.
    ///
    /// Ghostty prunes complete oldest pages. Consequently dead refs must be a
    /// prefix of `[boundary, start, end, ...]`: a fully dead interval no
    /// longer has surviving rows, while a dead start with a live end resumes
    /// exactly at the surviving top. Reset makes the whole sequence dead and
    /// therefore drops every now-irrelevant interval through the same rule.
    fn reconcile_exclusions(
        &mut self,
        vt: &mut Terminal<'_, '_>,
        boundary_repaired: bool,
    ) -> io::Result<()> {
        // Liveness must be non-decreasing along
        // [boundary, ex0.start, ex0.end, ex1.start, ex1.end, ...].
        let mut prev_alive = !boundary_repaired;
        for ex in &self.resize_exclusions {
            for alive in [ex.start.has_value(), ex.end.has_value()] {
                if !alive && prev_alive {
                    return Err(io::Error::other(
                        "resize exclusion pin lost without a preceding prune",
                    ));
                }
                prev_alive = alive;
            }
        }
        while let Some(front) = self.resize_exclusions.front_mut() {
            if !front.end.has_value() {
                // The entire interval was pruned; nothing it excluded
                // survives.
                tracing::warn!("resize exclusion fully pruned, dropping it");
                self.resize_exclusions.pop_front();
            } else if !front.start.has_value() {
                // Pruning stopped inside the interval, so the surviving top
                // is the first surviving excluded row: the interval resumes
                // there exactly.
                tracing::warn!("resize exclusion partially pruned, re-anchoring start");
                front
                    .start
                    .set(vt, screen_point(0))
                    .map_err(io::Error::other)?;
                break;
            } else {
                break;
            }
        }
        Ok(())
    }

    /// Resolve the flush boundary pin to its screen-space point. The pin is
    /// reconciled at every VT mutation point, so failing to resolve it here
    /// means renderer or libghostty state corruption.
    fn boundary_point(&self) -> io::Result<PointCoordinate> {
        self.flush_boundary
            .point(PointSpace::Screen)
            .map_err(io::Error::other)?
            .ok_or_else(|| io::Error::other("flush boundary pin unexpectedly lost"))
    }

    /// Resolve an exclusion to screen-space endpoints. Exclusion pins
    /// are reconciled at every VT mutation point, so failing to resolve one
    /// here means renderer or libghostty state corruption.
    fn exclusion_points(ex: &ResizeExclusion) -> io::Result<(PointCoordinate, PointCoordinate)> {
        let start = ex
            .start
            .point(PointSpace::Screen)
            .map_err(io::Error::other)?
            .ok_or_else(|| io::Error::other("resize exclusion start pin unexpectedly lost"))?;
        let end = ex
            .end
            .point(PointSpace::Screen)
            .map_err(io::Error::other)?
            .ok_or_else(|| io::Error::other("resize exclusion end pin unexpectedly lost"))?;
        Ok((start, end))
    }

    fn exclusion_rows(ex: &ResizeExclusion) -> io::Result<(usize, usize)> {
        let (start, end) = Self::exclusion_points(ex)?;
        Ok((start.y as usize, end.y as usize))
    }

    /// Record the interval of old-viewport rows that a primary-screen height
    /// shrink moved into history. Multiple shrinks queue multiple intervals;
    /// each is exact, so no interval ever needs to displace another.
    fn push_resize_exclusion(&mut self, start: TrackedGridRef, end: TrackedGridRef) {
        self.resize_exclusions
            .push_back(ResizeExclusion { start, end });
    }

    /// Number of VT scrollback rows not yet flushed to the native terminal:
    /// the scrollback region minus what precedes the flush boundary and
    /// minus the resize exclusions the native terminal already shows.
    ///
    /// Returns 0 on the alternate screen (it has no scrollback).
    fn unflushed(&self, vt: &Terminal<'_, '_>) -> io::Result<usize> {
        if vt.active_screen().ok() != Some(Screen::Primary) {
            return Ok(0);
        }
        let scrollback = vt.scrollback_rows().unwrap_or(0);
        let mut cursor = (self.boundary_point()?.y as usize).min(scrollback);
        let mut pending = 0usize;
        for ex in &self.resize_exclusions {
            let (start, end) = Self::exclusion_points(ex)?;
            pending += (start.y as usize).min(scrollback).saturating_sub(cursor);
            cursor = cursor.max((end.y as usize).min(scrollback));
        }
        Ok(pending + scrollback.saturating_sub(cursor))
    }

    /// Number of VT rows that fit on-screen below the takeover offset.
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
        self.viewport.write_terminal_row(stdout, vt, point)
    }

    /// Render changed VT lines to stdout. Skips lines that haven't changed
    /// and clips rows that would fall outside the visible area.
    ///
    /// Uses Ghostty's render-state dirty tracking to skip clean rows. Candidate
    /// rows are compared in Ghostty's canonical VT form with the native
    /// terminal baseline, suppressing conservative page-level dirty flags.
    fn render(&mut self, stdout: &mut impl Write, vt: &Terminal<'a, '_>) -> io::Result<()> {
        let max_row = self.visible_rows();
        let rows = vt.rows().unwrap_or(0) as usize;
        let visible = rows.min(max_row);
        let viewport_shifted = self.pending_scroll > 0;
        self.pending_scroll = 0;
        self.viewport
            .render(stdout, vt, visible, self.row_offset, viewport_shifted)?;
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
        let boundary = self.boundary_point()?;
        let start = boundary.y as usize;
        // Flush stops at the front exclusion; the rows past its end stay
        // pending and drain on the follow-up pass below.
        let exclusion = match self.resize_exclusions.front() {
            Some(ex) => Some(Self::exclusion_rows(ex)?),
            None => None,
        };
        let end = exclusion
            .map(|(ex_start, _)| ex_start.min(vt_scrollback))
            .unwrap_or(vt_scrollback);
        let mut flush_rows = end.saturating_sub(start.min(end));

        // A tracked point in the middle of a row means the boundary was
        // created by an older, unsafe reflow. Never emit that row wholesale:
        // its prefix may already be in native scrollback. New boundaries are
        // kept at logical-row starts below, so this is only a degraded case.
        let boundary_unsafe = boundary.x != 0
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

        // Boundary advancement must be failable before anything reaches the
        // native terminal. Ghostty's `TrackedGridRef::set` allocates a new pin
        // and can return OOM; doing that after emitting rows would leave the
        // old boundary in place and a retry would duplicate those rows.
        let resize_flush = exclusion.is_some();
        let planned_flush = !boundary_unsafe
            && ((flush_rows > 0 && self.content_rows > 0) || (flush_rows == 0 && resize_flush));
        let next_boundary = if planned_flush {
            let anchor = if let Some((_, ex_end)) = exclusion {
                screen_point(ex_end.min(vt_scrollback) as u32)
            } else if start + flush_rows < end {
                screen_point((start + flush_rows) as u32)
            } else {
                active_point(0)
            };
            Some(vt.track_grid_ref(anchor).map_err(io::Error::other)?)
        } else {
            None
        };

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

        let successful_flush = !boundary_unsafe && !incomplete && flushed_total == flush_rows;
        if successful_flush && (flushed_total > 0 || resize_flush) {
            // `planned_flush` is true for every successful state change, so
            // the replacement pin was allocated before the first output byte.
            self.flush_boundary = next_boundary
                .expect("successful scrollback flush must have a replacement boundary");

            // Rotate instead of draining so row allocations remain reusable.
            self.viewport.shift_cache(flushed_total);
            self.pending_scroll = self.pending_scroll.max(1);

            if resize_flush {
                self.resize_exclusions.pop_front();
                // Rows fed after the resize sit below the interval and are
                // still pending; drain them (and any further exclusions)
                // before presenting the viewport.
                return self.render_with_scroll(stdout, vt);
            }
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
        self.viewport.invalidate();
    }

    /// Start rendering on a terminal that may already have content above the
    /// cursor. With `offset > 0` that content stays on screen: the VT maps
    /// below it (snapshotting the native baseline so nothing repaints over
    /// it) and `make_room` scrolls it away only as the shell needs the
    /// space. With `offset == 0` the screen is ours; draw everything.
    fn init_takeover(
        &mut self,
        offset: u16,
        stdout: &mut impl Write,
        vt: &Terminal<'a, '_>,
    ) -> io::Result<()> {
        if offset > 0 {
            self.row_offset = offset;
            self.viewport.sync(vt);
            self.prev_cursor = CursorState::from_terminal(vt);
            Ok(())
        } else {
            self.render_full(stdout, vt)
        }
    }

    /// While pre-existing content sits above the takeover offset, scroll it
    /// into native scrollback as the shell needs the space: when the VT
    /// scrolled, when the cursor would land below the visible area, or all
    /// at once on alternate screen / explicit clear (CSI 2J).
    fn make_room(
        &mut self,
        stdout: &mut impl Write,
        vt: &Terminal<'_, '_>,
        esc: &EscapeState,
    ) -> io::Result<()> {
        if self.row_offset == 0 {
            return Ok(());
        }
        let cursor_row = vt.cursor_y().map(|r| r as usize).unwrap_or(0);
        let cursor_excess = (cursor_row + 1).saturating_sub(self.visible_rows());
        // `pending_scroll` spans every batch deferred by mode 2026; `feed`
        // accumulates it and only a presented render resets it.
        let need = self.pending_scroll.max(cursor_excess);
        let consumed = if esc.in_alternate_screen || esc.erase_display {
            // Alternate screen or explicit screen clear (CSI 2J): the shell
            // takes the full visible area.
            self.row_offset as usize
        } else {
            need.min(self.row_offset as usize)
        };
        if consumed > 0 {
            Self::scroll_region(stdout, self.content_rows, consumed)?;
            self.row_offset -= consumed as u16;
            self.invalidate();
        }
        Ok(())
    }

    /// Present the current VT state: scroll-flush the primary screen when the
    /// full area is ours, plain redraw while on the alternate screen or still
    /// rendering below pre-existing terminal content.
    fn present(&mut self, stdout: &mut impl Write, vt: &mut Terminal<'a, '_>) -> io::Result<()> {
        let primary = vt.active_screen().ok() == Some(Screen::Primary);
        if !primary || self.row_offset > 0 {
            self.render(stdout, vt)
        } else {
            self.render_with_scroll(stdout, vt)
        }
    }

    /// Terminal resize: the takeover phase ends (the terminal reflowed the
    /// old content itself) and the content area gets its new height.
    fn resized(&mut self, content_rows: u16) {
        self.row_offset = 0;
        self.content_rows = content_rows;
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

/// Shell session configuration.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Show status line at bottom of terminal.
    pub show_status_line: bool,
    /// Initial terminal size (auto-detected if None).
    pub size: Option<PtySize>,
    pub keybindings: ShellKeybindings,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            show_status_line: true,
            size: None,
            keybindings: ShellKeybindings::default(),
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

/// Per-render draining limit for the shared event FIFO.
///
/// The initial PTY event is accounted before entering the drain loop. Events
/// are only received while there is budget, so the next event remains in the
/// channel for the next render pass; this keeps the channel's ordering exact.
#[derive(Clone, Copy, Debug)]
struct RenderBatchBudget {
    event_count: usize,
    pty_bytes: usize,
}

impl RenderBatchBudget {
    fn for_initial_pty_output(bytes: usize) -> Self {
        Self {
            event_count: 1,
            pty_bytes: bytes,
        }
    }

    fn can_drain(&self) -> bool {
        self.event_count < MAX_RENDER_BATCH_EVENTS && self.pty_bytes < MAX_PTY_BATCH_BYTES
    }

    fn record(&mut self, event: &Event) {
        self.event_count += 1;
        if let Event::PtyOutput(bytes) = event {
            self.pty_bytes += bytes.len();
        }
    }

    fn try_recv(&mut self, event_rx: &std::sync::mpsc::Receiver<Event>) -> Option<Event> {
        if !self.can_drain() {
            return None;
        }
        let event = event_rx.try_recv().ok()?;
        self.record(&event);
        Some(event)
    }
}

fn return_pty_read_buffer(buffer_tx: &std::sync::mpsc::SyncSender<Vec<u8>>, mut buffer: Vec<u8>) {
    buffer.clear();
    // A normal PTY event came from this pool and therefore frees a slot here.
    // Use a non-blocking return nevertheless: synthetic/future events must
    // never stall the renderer, and shutdown simply drops the spare buffer.
    let _ = buffer_tx.try_send(buffer);
}

/// Dependencies owned by the render thread's event loop. Keeping them
/// together makes their ownership boundary explicit: only the receiver moves
/// into the loop; terminal-facing state remains borrowed on that thread.
struct EventLoopContext<'a> {
    pty: &'a Arc<Pty>,
    event_rx: std::sync::mpsc::Receiver<Event>,
    coordinator_tx: &'a tokio_mpsc::Sender<FrontendEvent>,
    stdout: &'a mut Box<dyn Write + Send>,
    virtual_pty_replies: &'a RefCell<VirtualPtyReplies>,
    pty_buffer_return_tx: &'a std::sync::mpsc::SyncSender<Vec<u8>>,
}

/// The terminal-facing dependencies needed to dispatch one stdin event. This
/// is shared by the top-level and batched paths so local keybindings never
/// become ordinary child input merely because PTY output arrived first.
struct StdinEventContext<'a, 'vt, 'cb> {
    pty: &'a Arc<Pty>,
    coordinator_tx: &'a tokio_mpsc::Sender<FrontendEvent>,
    stdout: &'a mut Box<dyn Write + Send>,
    vt: &'a mut Terminal<'vt, 'cb>,
    renderer: &'a mut Renderer<'vt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StdinDisposition {
    ForwardToPty,
    TogglePause,
    ListWatchedFiles,
    ToggleError,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StdinPresentation {
    /// The event arrived on its own, so preserve the established immediate
    /// error-panel rendering behavior.
    Immediate,
    /// The event was drained behind PTY output; let the surrounding batch
    /// present exactly one atomic frame instead.
    Deferred,
}

fn classify_stdin(
    keybindings: &ShellKeybindings,
    data: &[u8],
    local_keybindings_enabled: bool,
) -> StdinDisposition {
    if !local_keybindings_enabled {
        return StdinDisposition::ForwardToPty;
    }

    match keybindings.action_for_input(data) {
        Some(ShellAction::TogglePause) => StdinDisposition::TogglePause,
        Some(ShellAction::ListWatchedFiles) => StdinDisposition::ListWatchedFiles,
        Some(ShellAction::ToggleError) => StdinDisposition::ToggleError,
        Some(ShellAction::Reload) | None => StdinDisposition::ForwardToPty,
    }
}

fn present_stdin_error_immediately(presentation: StdinPresentation, synchronized: bool) -> bool {
    presentation == StdinPresentation::Immediate && !synchronized
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
        status_line.set_keybindings(config.keybindings.clone());

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

    pub fn with_keybindings(mut self, keybindings: ShellKeybindings) -> Self {
        self.status_line.set_keybindings(keybindings.clone());
        self.config.keybindings = keybindings;
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
    /// * `command_rx` - Receives commands from the frontend mailbox
    /// * `event_tx` - Sends events to the frontend mailbox
    pub async fn run(
        mut self,
        mut command_rx: tokio_mpsc::Receiver<FrontendCommand>,
        event_tx: tokio_mpsc::Sender<FrontendEvent>,
        io: SessionIo,
    ) -> Result<Option<u32>, SessionError> {
        // Wait for the initial Spawn command. The renderer can hand terminal
        // ownership over just before the backend fails, so cancellation must
        // be observed even while the mailbox remains open without a Spawn.
        let initial_command = match &self.shutdown_token {
            Some(token) => {
                tokio::select! {
                    command = command_rx.recv() => command,
                    _ = token.cancelled() => return Ok(None),
                }
            }
            None => command_rx.recv().await,
        };
        let (initial_cmd, _watch_files) = match initial_command {
            Some(FrontendCommand::Shell(ShellCommand::Spawn {
                command,
                watch_files,
            })) => {
                self.status_line
                    .state_mut()
                    .set_watched_file_count(watch_files.len());
                (command, watch_files)
            }
            Some(FrontendCommand::Shell(ShellCommand::Shutdown)) | None => {
                return Ok(None);
            }
            Some(other) => {
                return Err(SessionError::UnexpectedCommand(format!("{:?}", other)));
            }
        };

        // Spawn PTY
        // Reserve 1 row for status line if enabled
        let pty_size = self.pty_size();

        let pty = Arc::new(Pty::spawn(initial_cmd, pty_size)?);

        // Enter raw mode
        tracing::trace!("session: entering raw mode");
        let _raw_guard = RawModeGuard::new()?;
        tracing::trace!("session: raw mode active");

        let injected_stdin = io.stdin.is_some();
        let stdout_raw: Box<dyn Write + Send> = io.stdout.unwrap_or_else(|| Box::new(io::stdout()));
        let mut stdout: Box<dyn Write + Send> = Box::new(io::BufWriter::new(stdout_raw));
        let stdin_source: Box<dyn Read + Send> = io.stdin.unwrap_or_else(|| Box::new(io::stdin()));

        // Ask the terminal where its cursor is — everything above it (build
        // summary, prior output) stays on screen, and the renderer starts
        // below. Done FIRST, before any terminal resets. Skip when stdin is
        // injected (not a real terminal) — the DSR response comes via stdin,
        // so this would hang if stdin is not a TTY. crossterm handles the
        // query, parsing, and a built-in 2s timeout for environments that
        // don't respond (Docker, CI).
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
        tracing::trace!("session: cursor position at takeover: row {}", cursor_row);

        // Renderers may leave a non-default scroll region/origin mode.
        // Reset both before we start cursor-addressed rendering, otherwise
        // the first shell draw can land in the wrong area and overlap
        // existing output.
        queue!(stdout, ResetScrollRegion, ResetDecMode(ORIGIN_MODE))?;
        stdout.flush()?;

        // Get terminal size.
        // TODO: query the size from the actual stdout fd (e.g. TIOCGWINSZ on the
        // writer) instead of crossterm::terminal::size() which always uses the
        // process's controlling terminal. That would make this work correctly even
        // with injected I/O and remove the need for the config.size guard.
        //
        // Like `get_terminal_size()`, this can come back `Ok` with a `0x0` size
        // (observed transiently under WSL2, and consistently in some
        // non-terminal-backed ptys). Ignore that instead of clobbering the
        // already-valid size `ShellSession::new` computed, since a `0` in
        // either dimension later crashes `libghostty_vt::Terminal::new` with
        // "invalid value".
        if self.config.size.is_none()
            && let Ok((cols, rows)) = terminal::size()
            && cols != 0
            && rows != 0
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
        // The renderer alone knows how to draw around the existing content.
        let takeover_offset = cursor_row.saturating_sub(1);
        let pty_size = self.pty_size();
        let _ = pty.resize(pty_size);

        // Set up event channel
        let (event_tx_internal, event_rx_internal) = std::sync::mpsc::channel::<Event>();

        // PTY output is the only high-volume source. Recycle a small fixed
        // buffer set through the renderer so its queue is bounded without
        // ever blocking async command forwarding on the shared event channel.
        let (pty_buffer_return_tx, pty_buffer_rx) =
            std::sync::mpsc::sync_channel::<Vec<u8>>(PTY_READ_BUFFER_COUNT);
        for _ in 0..PTY_READ_BUFFER_COUNT {
            pty_buffer_return_tx
                .send(vec![0; PTY_READ_BUFFER_BYTES])
                .expect("fresh PTY buffer pool has capacity");
        }
        let pty_buffer_return_tx_for_loop = pty_buffer_return_tx.clone();

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
                while let Ok(mut buf) = pty_buffer_rx.recv() {
                    // When all buffers are awaiting rendering, wait here
                    // rather than allocating another event payload. Dropping
                    // the renderer's return sender disconnects this receive
                    // during session shutdown.
                    buf.resize(PTY_READ_BUFFER_BYTES, 0);
                    match pty_reader.read(&mut buf) {
                        Ok(0) => {
                            let exit_code = pty_reader.wait_for_exit().map(|s| s.exit_code());
                            let _ = pty_tx.send(Event::PtyExit(exit_code));
                            break;
                        }
                        Ok(n) => {
                            buf.truncate(n);
                            if pty_tx.send(Event::PtyOutput(buf)).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("session: PTY read error: {}", e);
                            let exit_code = pty_reader.wait_for_exit().map(|s| s.exit_code());
                            let _ = pty_tx.send(Event::PtyExit(exit_code));
                            break;
                        }
                    }
                }
            })
            .expect("failed to spawn session-pty thread");

        // The renderer keeps the sole return sender. Once it exits, the PTY
        // reader wakes from `recv` and exits instead of waiting for a buffer
        // that can no longer be returned.
        drop(pty_buffer_return_tx);

        // Forward coordinator commands to internal event channel
        let cmd_tx = event_tx_internal.clone();
        tokio::spawn(async move {
            while let Some(command) = command_rx.recv().await {
                let FrontendCommand::Shell(command) = command else {
                    tracing::debug!(?command, "ignoring frontend command after shell takeover");
                    continue;
                };
                if cmd_tx.send(Event::Command(command)).is_err() {
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
            // libghostty-vt invokes effects synchronously from `vt_write`.
            // This buffer therefore remains thread-local with the terminal;
            // it is drained into the child PTY immediately after each feed.
            let virtual_pty_replies = RefCell::new(VirtualPtyReplies::default());
            // Create the VT on this thread (Terminal is !Send)
            let mut vt = Terminal::new(pty_size.cols, pty_size.rows)?;
            vt.set_scrollback_max_bytes(Some(DEFAULT_MAX_SCROLLBACK))?;
            vt.on_pty_write({
                let virtual_pty_replies = &virtual_pty_replies;
                move |_term, data| virtual_pty_replies.borrow_mut().capture(data)
            })?;

            // Reset the VT after resize so any stale PTY output (the shell's
            // PROMPT_COMMAND after task execution, SIGWINCH redraw from the
            // resize above) starts on a clean slate. The event loop will
            // process any pending PTY output normally.
            if let Err(e) = vt.resize(pty_size.cols, pty_size.rows, 0, 0) {
                tracing::warn!("failed to resize terminal: {e}");
            }
            vt.vt_write(b"\x1b[2J\x1b[H");

            // Initialize the renderer; content above the takeover row stays.
            let mut renderer = Renderer::new(pty_size.rows, &vt)?;
            renderer.init_takeover(takeover_offset, &mut stdout, &vt)?;
            if self.config.show_status_line {
                self.status_line
                    .draw(&mut stdout, self.size.cols, self.size.rows)?;
            }
            renderer.write_cursor(&mut stdout, &vt)?;
            stdout.flush()?;

            self.event_loop(
                &mut vt,
                &mut renderer,
                EventLoopContext {
                    pty: &pty_for_thread,
                    event_rx: event_rx_internal,
                    coordinator_tx: &coordinator_tx,
                    stdout: &mut stdout,
                    virtual_pty_replies: &virtual_pty_replies,
                    pty_buffer_return_tx: &pty_buffer_return_tx_for_loop,
                },
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
        if let Err(e) = event_tx
            .send(FrontendEvent::Shell(ShellEvent::Exited { exit_code }))
            .await
        {
            tracing::trace!("failed to send Exited event: {e}");
        }

        Ok(exit_code)
    }

    /// Main event loop handling stdin, PTY output, and coordinator commands.
    /// Returns the exit code from the PTY child process, if available.
    fn event_loop<'a>(
        &mut self,
        vt: &mut Terminal<'a, '_>,
        renderer: &mut Renderer<'a>,
        context: EventLoopContext<'_>,
    ) -> Result<Option<u32>, SessionError> {
        let EventLoopContext {
            pty,
            event_rx,
            coordinator_tx,
            stdout,
            virtual_pty_replies,
            pty_buffer_return_tx,
        } = context;
        let spinner_interval = Duration::from_millis(SPINNER_INTERVAL_MS);
        let mut scanner = EscapeScanner::new();
        let mut vt_input_filter = VtInputFilter::new();
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
                    self.dispatch_stdin_event(
                        &data,
                        !esc.in_alternate_screen,
                        StdinPresentation::Immediate,
                        StdinEventContext {
                            pty,
                            coordinator_tx,
                            stdout,
                            vt,
                            renderer,
                        },
                    )?;
                }

                Event::PtyOutput(data) => {
                    let mut batch_budget = RenderBatchBudget::for_initial_pty_output(data.len());
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

                    // Feed output into VT; `feed` tracks scrolled-off lines
                    // in the renderer's own `pending_scroll`.
                    {
                        let filtered = vt_input_filter.filter(&data, &mut vt_input);
                        renderer.feed(vt, filtered.as_bytes())?;
                    }
                    flush_virtual_pty_replies(virtual_pty_replies, pty)?;
                    return_pty_read_buffer(pty_buffer_return_tx, data);

                    // Bound this render batch so sustained PTY output cannot
                    // starve presentation or leave the next control event
                    // behind an unbounded drain. The FIFO remains intact:
                    // once exhausted, the next event is not received yet.
                    while let Some(event) = batch_budget.try_recv(&event_rx) {
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
                                {
                                    let filtered = vt_input_filter.filter(&more, &mut vt_input);
                                    renderer.feed(vt, filtered.as_bytes())?;
                                }
                                flush_virtual_pty_replies(virtual_pty_replies, pty)?;
                                return_pty_read_buffer(pty_buffer_return_tx, more);
                            }
                            Event::PtyExit(exit_code) => {
                                let synchronized = synchronized_output_active(vt);
                                let deferred_release = esc.has_deferred_synchronized_output_reset();
                                self.clear_status_row(stdout, esc.in_alternate_screen)?;
                                escape_state_cleanup(&esc, stdout)?;
                                if !synchronized && !deferred_release {
                                    begin_renderer_transaction(stdout, false)?;
                                }
                                renderer.render_with_scroll(stdout, vt)?;
                                finish_renderer_transaction(stdout, &mut esc, true)?;
                                stdout.flush()?;
                                return Ok(exit_code);
                            }
                            Event::Stdin(stdin_data) => {
                                self.dispatch_stdin_event(
                                    &stdin_data,
                                    !esc.in_alternate_screen,
                                    StdinPresentation::Deferred,
                                    StdinEventContext {
                                        pty,
                                        coordinator_tx,
                                        stdout,
                                        vt,
                                        renderer,
                                    },
                                )?;
                            }
                            Event::Command(ShellCommand::Shutdown) => {
                                let synchronized = synchronized_output_active(vt);
                                let deferred_release = esc.has_deferred_synchronized_output_reset();
                                self.clear_status_row(stdout, esc.in_alternate_screen)?;
                                escape_state_cleanup(&esc, stdout)?;
                                if !synchronized && !deferred_release {
                                    begin_renderer_transaction(stdout, false)?;
                                }
                                renderer.render_with_scroll(stdout, vt)?;
                                finish_renderer_transaction(stdout, &mut esc, true)?;
                                stdout.flush()?;
                                return Ok(None);
                            }
                            Event::Command(cmd) => {
                                self.handle_command(cmd, vt, renderer)?;
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

                    // The application has released its virtual mode-2026
                    // transaction, but `EscapeState` intentionally kept the
                    // physical reset pending. Render into that same physical
                    // transaction, then make the reset the final byte.
                    let deferred_release = esc.has_deferred_synchronized_output_reset();

                    // Begin synchronized output so the terminal buffers
                    // all writes atomically (mode 2026).
                    begin_renderer_transaction(stdout, deferred_release)?;

                    // Handle alternate screen transitions
                    if was_in_alt != esc.in_alternate_screen {
                        renderer.invalidate();
                    }

                    renderer.make_room(stdout, vt, &esc)?;

                    if esc.clear_scrollback {
                        queue!(stdout, Clear(ClearType::Purge))?;
                    }

                    renderer.present(stdout, vt)?;

                    if self.config.show_status_line {
                        self.status_line
                            .draw(stdout, self.size.cols, self.size.rows)?;
                    }
                    renderer.write_cursor(stdout, vt)?;

                    // End synchronized output and flush. For an
                    // application-owned transaction, its original reset is
                    // deliberately emitted only after the completed frame.
                    finish_renderer_transaction(stdout, &mut esc, true)?;
                    stdout.flush()?;
                }

                Event::PtyExit(exit_code) => {
                    let synchronized = synchronized_output_active(vt);
                    let deferred_release = esc.has_deferred_synchronized_output_reset();
                    if synchronized {
                        // Present the final deferred frame before releasing
                        // mode 2026, even if the child exits without doing so.
                        renderer.render_with_scroll(stdout, vt)?;
                    }
                    self.clear_status_row(stdout, esc.in_alternate_screen)?;
                    escape_state_cleanup(&esc, stdout)?;
                    finish_renderer_transaction(
                        stdout,
                        &mut esc,
                        synchronized || deferred_release,
                    )?;
                    stdout.flush()?;
                    return Ok(exit_code);
                }

                Event::Command(ShellCommand::Shutdown) => break,

                Event::Command(cmd) => {
                    self.handle_command(cmd, vt, renderer)?;
                    if synchronized_output_active(vt) {
                        continue;
                    }
                    queue!(stdout, terminal::BeginSynchronizedUpdate)?;
                    renderer.present(stdout, vt)?;
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
                        // has already retained those same rows, so pin the
                        // old active top before mutating the VT grid; with
                        // the new active top pinned after the resize it
                        // bounds the excluded interval exactly.
                        let old_native_rows = self.size.rows;
                        let shrinking_primary =
                            primary_height_shrunk(old_native_rows, rows, vt.active_screen().ok());
                        self.size = PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        };
                        let exclusion_start = if shrinking_primary {
                            Some(vt.track_grid_ref(active_point(0))?)
                        } else {
                            None
                        };
                        let pty_size = self.pty_size();
                        renderer.resized(pty_size.rows);
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
                        } else {
                            if let Some(start) = exclusion_start {
                                let end = vt.track_grid_ref(active_point(0))?;
                                renderer.push_resize_exclusion(start, end);
                            }
                            renderer.reconcile_after_mutation(vt)?;
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
                        if let Err(e) =
                            coordinator_tx.try_send(FrontendEvent::Shell(ShellEvent::Resize {
                                cols: pty_size.cols,
                                rows: pty_size.rows,
                            }))
                        {
                            tracing::trace!("failed to send Resize event: {e}");
                        }
                    }
                }
            }
        }

        let synchronized = synchronized_output_active(vt);
        let deferred_release = esc.has_deferred_synchronized_output_reset();
        if synchronized {
            renderer.render_with_scroll(stdout, vt)?;
        }
        self.clear_status_row(stdout, esc.in_alternate_screen)?;
        escape_state_cleanup(&esc, stdout)?;
        finish_renderer_transaction(stdout, &mut esc, synchronized || deferred_release)?;
        stdout.flush()?;
        Ok(None)
    }

    fn dispatch_stdin_event(
        &mut self,
        data: &[u8],
        local_keybindings_enabled: bool,
        presentation: StdinPresentation,
        context: StdinEventContext<'_, '_, '_>,
    ) -> Result<(), SessionError> {
        let StdinEventContext {
            pty,
            coordinator_tx,
            stdout,
            vt,
            renderer,
        } = context;
        match classify_stdin(&self.config.keybindings, data, local_keybindings_enabled) {
            StdinDisposition::TogglePause => {
                if let Err(e) =
                    coordinator_tx.try_send(FrontendEvent::Shell(ShellEvent::TogglePause))
                {
                    tracing::trace!("failed to send TogglePause event: {e}");
                }
            }
            StdinDisposition::ListWatchedFiles => {
                if let Err(e) =
                    coordinator_tx.try_send(FrontendEvent::Shell(ShellEvent::ListWatchedFiles))
                {
                    tracing::trace!("failed to send ListWatchedFiles event: {e}");
                }
            }
            StdinDisposition::ToggleError => {
                let state = self.status_line.state_mut();
                if state.error.is_some() {
                    state.show_error = !state.show_error;
                    let synchronized = synchronized_output_active(vt);
                    if state.show_error {
                        let error = state.error.clone().unwrap();
                        let mut error_text = String::from("\r\n\x1b[1;31mBuild error:\x1b[0m\r\n");
                        for line in error.lines() {
                            error_text.push_str(&format!("  {}\r\n", line));
                        }
                        error_text.push_str("\r\n");
                        renderer.feed(vt, &error_text)?;
                        if present_stdin_error_immediately(presentation, synchronized) {
                            renderer.present(stdout, vt)?;
                        }
                    } else {
                        pty.write_all(&[0x0C])?;
                        pty.flush()?;
                    }
                    if present_stdin_error_immediately(presentation, synchronized) {
                        self.status_line
                            .draw(stdout, self.size.cols, self.size.rows)?;
                        renderer.write_cursor(stdout, vt)?;
                        stdout.flush()?;
                    }
                }
            }
            StdinDisposition::ForwardToPty if !data.is_empty() => {
                pty.write_all(data)?;
                pty.flush()?;
            }
            StdinDisposition::ForwardToPty => {}
        }
        Ok(())
    }

    /// Handle a command from the coordinator.
    ///
    /// Updates state and, for some commands (e.g. `PrintWatchedFiles`), feeds
    /// text into the VT. Does not write to stdout.
    fn handle_command(
        &mut self,
        cmd: ShellCommand,
        vt: &mut Terminal<'_, '_>,
        renderer: &mut Renderer<'_>,
    ) -> Result<(), SessionError> {
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
                renderer.feed(vt, &text)?;
            }

            ShellCommand::Shutdown => unreachable!("shutdown is handled by the event loop"),

            ShellCommand::Spawn { .. } => {
                // Shouldn't receive Spawn after initial
            }
        }

        Ok(())
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

    fn pty_output_byte(event: Event) -> u8 {
        match event {
            Event::PtyOutput(bytes) => *bytes.first().expect("non-empty PTY output"),
            _ => panic!("expected PTY output event"),
        }
    }

    #[test]
    fn deferred_application_sync_reset_is_the_presentation_boundary() {
        let mut esc = EscapeState::new();
        let set = crate::escape::DecModeEvent::Set {
            modes: vec![2026],
            raw_bytes: b"\x1b[?2026h".to_vec(),
        };
        let reset = crate::escape::DecModeEvent::Reset {
            modes: vec![2026],
            raw_bytes: b"\x1b[?2026l".to_vec(),
        };
        let mut stdout = Vec::new();
        stdout.extend_from_slice(esc.apply_dec_mode(&set));
        assert!(esc.apply_dec_mode(&reset).is_empty());

        // This helper is used after renderer/status/cursor output. It must
        // not insert its own mode-2026 h/l pair inside the application's
        // transaction.
        stdout.extend_from_slice(b"<frame><status><cursor>");
        finish_renderer_transaction(&mut stdout, &mut esc, true).unwrap();
        assert_eq!(stdout, b"\x1b[?2026h<frame><status><cursor>\x1b[?2026l");
    }

    #[test]
    fn render_batch_budget_caps_event_count_and_leaves_the_next_event_queued() {
        let (tx, rx) = std::sync::mpsc::channel();
        let output_count = MAX_RENDER_BATCH_EVENTS * 3;
        for byte in 1..output_count as u8 {
            tx.send(Event::PtyOutput(vec![byte])).unwrap();
        }

        // The first PTY event is the one that entered the outer event loop.
        let mut budget = RenderBatchBudget::for_initial_pty_output(1);
        let mut rendered = vec![0];
        while let Some(event) = budget.try_recv(&rx) {
            rendered.push(pty_output_byte(event));
        }

        // The initial event counts toward the event budget. No later PTY
        // event has been read early, so it will start the following frame.
        assert_eq!(
            rendered,
            (0..MAX_RENDER_BATCH_EVENTS as u8).collect::<Vec<_>>()
        );
        assert_eq!(
            pty_output_byte(rx.try_recv().unwrap()),
            MAX_RENDER_BATCH_EVENTS as u8
        );

        // Everything after that boundary remains ordered and intact.
        let remaining: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
            .map(pty_output_byte)
            .collect();
        assert_eq!(
            remaining,
            ((MAX_RENDER_BATCH_EVENTS + 1) as u8..output_count as u8).collect::<Vec<_>>()
        );
    }

    #[test]
    fn render_batch_budget_caps_pty_bytes_before_reading_the_next_chunk() {
        let (tx, rx) = std::sync::mpsc::channel();
        let chunk_len = MAX_PTY_BATCH_BYTES / 8;
        for byte in 1..10 {
            tx.send(Event::PtyOutput(vec![byte; chunk_len])).unwrap();
        }

        let mut budget = RenderBatchBudget::for_initial_pty_output(chunk_len);
        let mut rendered = vec![0];
        while let Some(event) = budget.try_recv(&rx) {
            rendered.push(pty_output_byte(event));
        }

        // Initial + seven queued chunks reaches exactly 32 KiB. The eighth
        // queued chunk remains unread for the next render pass.
        assert_eq!(rendered, (0..8).collect::<Vec<_>>());
        assert_eq!(pty_output_byte(rx.try_recv().unwrap()), 8);
    }

    #[test]
    fn render_batch_budget_keeps_interleaved_control_events_in_fifo_order() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Event::PtyOutput(b"b".to_vec())).unwrap();
        tx.send(Event::Stdin(b"input".to_vec())).unwrap();
        tx.send(Event::Command(ShellCommand::ReloadApplied))
            .unwrap();
        tx.send(Event::Resize).unwrap();
        tx.send(Event::PtyOutput(b"c".to_vec())).unwrap();

        let mut budget = RenderBatchBudget::for_initial_pty_output(1);
        assert_eq!(pty_output_byte(budget.try_recv(&rx).unwrap()), b'b');
        assert!(matches!(budget.try_recv(&rx), Some(Event::Stdin(data)) if data == b"input"));
        assert!(matches!(
            budget.try_recv(&rx),
            Some(Event::Command(ShellCommand::ReloadApplied))
        ));

        // This matches the event-loop branch: defer exactly one resize, then
        // stop draining. The following PTY bytes are untouched and therefore
        // cannot be dropped or reordered around the resize.
        assert!(matches!(budget.try_recv(&rx), Some(Event::Resize)));
        assert_eq!(pty_output_byte(rx.try_recv().unwrap()), b'c');
    }

    #[test]
    fn batched_stdin_keybind_is_local_not_pty_input() {
        let (tx, rx) = std::sync::mpsc::channel();
        let keybindings = ShellKeybindings::default();
        let toggle_pause = keybindings.bindings(ShellAction::TogglePause)[0].terminal_bytes();
        tx.send(Event::Stdin(toggle_pause)).unwrap();
        tx.send(Event::PtyOutput(b"later".to_vec())).unwrap();

        // Model the pending PTY output that opened a render batch, then the
        // keybinding interleaved behind it in the shared FIFO.
        let mut budget = RenderBatchBudget::for_initial_pty_output(b"first".len());
        let event = budget.try_recv(&rx).unwrap();
        let Event::Stdin(data) = event else {
            panic!("expected the interleaved stdin event");
        };

        let mut pty_writes = Vec::new();
        let mut shell_events = Vec::new();
        match classify_stdin(&keybindings, &data, true) {
            StdinDisposition::TogglePause => shell_events.push(ShellEvent::TogglePause),
            StdinDisposition::ForwardToPty => pty_writes.extend_from_slice(&data),
            other => panic!("unexpected keybinding disposition: {other:?}"),
        }

        assert!(pty_writes.is_empty());
        assert!(matches!(shell_events.as_slice(), [ShellEvent::TogglePause]));
        assert_eq!(pty_output_byte(rx.try_recv().unwrap()), b'l');
    }

    #[test]
    fn configurable_stdin_keybindings_replace_and_disable_defaults() {
        let mut keybindings = ShellKeybindings::default();
        keybindings.replace(
            ShellAction::TogglePause,
            vec![crate::keybindings::ShellKeyChord::new(
                crate::keybindings::ShellKeyCode::Function(12),
                false,
                false,
                false,
            )],
        );
        keybindings.replace(
            ShellAction::ListWatchedFiles,
            vec![crate::keybindings::ShellKeyChord::new(
                crate::keybindings::ShellKeyCode::Char('l'),
                false,
                true,
                false,
            )],
        );
        keybindings.replace(ShellAction::ToggleError, Vec::new());

        assert_eq!(
            classify_stdin(&keybindings, b"\x1b[24~", true),
            StdinDisposition::TogglePause
        );
        assert_eq!(
            classify_stdin(&keybindings, &[0x1b, 0x04], true),
            StdinDisposition::ForwardToPty
        );
        assert_eq!(
            classify_stdin(&keybindings, &[0x1b, b'l'], true),
            StdinDisposition::ListWatchedFiles
        );
        assert_eq!(
            classify_stdin(&keybindings, &[0x1b, 0x17], true),
            StdinDisposition::ForwardToPty
        );
        assert_eq!(
            classify_stdin(&keybindings, &[0x1b, 0x05], true),
            StdinDisposition::ForwardToPty
        );
    }

    #[test]
    fn configurable_stdin_keybindings_are_forwarded_in_alternate_screen() {
        let mut keybindings = ShellKeybindings::default();
        keybindings.replace(
            ShellAction::TogglePause,
            vec![crate::keybindings::ShellKeyChord::new(
                crate::keybindings::ShellKeyCode::Function(12),
                false,
                false,
                false,
            )],
        );

        assert_eq!(
            classify_stdin(&keybindings, b"\x1b[24~", true),
            StdinDisposition::TogglePause
        );
        assert_eq!(
            classify_stdin(&keybindings, b"\x1b[24~", false),
            StdinDisposition::ForwardToPty
        );
    }

    #[test]
    fn batched_stdin_error_toggle_defers_presentation_to_the_pty_frame() {
        assert!(present_stdin_error_immediately(
            StdinPresentation::Immediate,
            false
        ));
        assert!(!present_stdin_error_immediately(
            StdinPresentation::Deferred,
            false
        ));
        assert!(!present_stdin_error_immediately(
            StdinPresentation::Immediate,
            true
        ));
    }

    #[test]
    fn pty_read_buffer_pool_reuses_buffers_and_disconnects_cleanly() {
        let (return_tx, return_rx) =
            std::sync::mpsc::sync_channel::<Vec<u8>>(PTY_READ_BUFFER_COUNT);
        for _ in 0..PTY_READ_BUFFER_COUNT {
            return_tx.send(vec![0; PTY_READ_BUFFER_BYTES]).unwrap();
        }

        let mut leased: Vec<_> = (0..PTY_READ_BUFFER_COUNT)
            .map(|_| return_rx.recv().unwrap())
            .collect();
        assert!(matches!(
            return_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));

        let buffer = leased.pop().unwrap();
        let pointer = buffer.as_ptr();
        return_pty_read_buffer(&return_tx, buffer);
        let recycled = return_rx.recv().unwrap();
        assert_eq!(recycled.as_ptr(), pointer);
        assert_eq!(recycled.capacity(), PTY_READ_BUFFER_BYTES);

        drop(recycled);
        drop(leased);
        drop(return_tx);
        assert!(matches!(return_rx.recv(), Err(std::sync::mpsc::RecvError)));
    }

    fn test_vt_with_size<'cb>(
        cols: u16,
        rows: u16,
        max_scrollback: usize,
    ) -> Terminal<'static, 'cb> {
        let mut vt = Terminal::new(cols, rows).expect("terminal");
        vt.set_scrollback_max_bytes(Some(max_scrollback))
            .expect("set scrollback limit");
        vt
    }

    fn test_vt<'cb>(max_scrollback: usize) -> Terminal<'static, 'cb> {
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
            keybindings: ShellKeybindings::default(),
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
    fn renderer_flush_boundary_accounting() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        // Fill the viewport: nothing scrolled off yet.
        for i in 0..ROWS as usize - 1 {
            assert_eq!(renderer.feed(&mut vt, format!("line{}\r\n", i)).unwrap(), 0);
        }
        assert_eq!(renderer.feed(&mut vt, "line4").unwrap(), 0);
        assert_eq!(renderer.unflushed(&vt).unwrap(), 0);

        // Each further line scrolls exactly one row into history.
        assert_eq!(renderer.feed(&mut vt, "\r\nline5").unwrap(), 1);
        assert_eq!(renderer.feed(&mut vt, "\r\nline6").unwrap(), 1);
        assert_eq!(renderer.unflushed(&vt).unwrap(), 2);

        // A flush consumes the unflushed region and re-anchors the pin.
        out.clear();
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        assert_eq!(renderer.unflushed(&vt).unwrap(), 0);

        // VT history is retained (no CSI 3J wipe), yet not re-flushed.
        assert_eq!(vt.scrollback_rows().unwrap(), 2);
        assert_eq!(renderer.feed(&mut vt, "\r\nline7").unwrap(), 1);
        assert_eq!(renderer.unflushed(&vt).unwrap(), 1);
    }

    #[test]
    fn renderer_flush_is_incremental() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        for i in 0..8 {
            renderer.feed(&mut vt, format!("line{}\r\n", i)).unwrap();
        }
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        for i in 8..12 {
            renderer.feed(&mut vt, format!("line{}\r\n", i)).unwrap();
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
            renderer.feed(&mut vt, format!("line{}\r\n", i)).unwrap();
        }
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        assert_eq!(renderer.unflushed(&vt).unwrap(), 0);

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
        renderer.feed(&mut vt, "after\r\n").unwrap();
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
        renderer.feed(&mut vt, "hello\r\nworld").unwrap();

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
    fn frame_plan_only_diffs_full_frames_caused_by_viewport_motion() {
        assert!(matches!(
            FramePlan::for_snapshot(Dirty::Full, true, true, true, false),
            FramePlan::CompareRows
        ));
        assert!(matches!(
            FramePlan::for_snapshot(Dirty::Full, true, true, true, true),
            FramePlan::Redraw
        ));
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
        renderer.feed(&mut vt, "\x1b[?1049h").unwrap();
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
    fn renderer_invalidation_reuses_render_buffers() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        renderer
            .feed(&mut vt, "line0\r\nline1\r\nline2\r\nline3\r\nline4")
            .unwrap();

        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();
        let capacities = renderer.viewport.retained_capacities();

        renderer.invalidate();
        out.clear();
        renderer.render(&mut out, &vt).unwrap();

        assert_eq!(renderer.viewport.retained_capacities(), capacities);
    }

    #[test]
    fn renderer_redraws_only_changed_row() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        for i in 0..ROWS as usize - 1 {
            renderer.feed(&mut vt, format!("line{}\r\n", i)).unwrap();
        }
        renderer.feed(&mut vt, "line4").unwrap();
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        // Overwrite a single character on row 0 (no scroll).
        renderer.feed(&mut vt, "\x1b[1;1HX").unwrap();
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
        renderer
            .feed(&mut vt, "\x1b[4:3;58;2;1;2;3;8;9;53me\u{301}\x1b[0m")
            .unwrap();

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
    fn renderer_repaints_style_only_changes_with_native_row_formatting() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();

        renderer.feed(&mut vt, "A").unwrap();
        renderer.render_full(&mut out, &vt).unwrap();

        // The character itself is unchanged. The encoded-row cache must
        // still observe Ghostty's native VT representation of the new style.
        renderer
            .feed(&mut vt, "\x1b[1;1H\x1b[38;2;1;2;3mA\x1b[0m")
            .unwrap();
        out.clear();
        renderer.render(&mut out, &vt).unwrap();

        assert_eq!(out.windows(4).filter(|w| *w == b"\x1b[2K").count(), 1);
        let mut replay = test_vt(DEFAULT_MAX_SCROLLBACK);
        replay.vt_write(&out);
        assert_eq!(row_plain_text(&replay, active_point(0)).trim_end(), "A");
        assert_eq!(
            replay.grid_ref(active_point(0)).unwrap().style().unwrap(),
            vt.grid_ref(active_point(0)).unwrap().style().unwrap()
        );
    }

    #[test]
    fn renderer_preserves_el_background_on_blank_rows() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);

        // EL under a non-default background is how Neovim paints otherwise
        // blank rows. Ghostty stores the erased suffix as background-only
        // cells rather than styled spaces.
        renderer
            .feed(&mut vt, "\x1b[48;2;1;2;3m\x1b[K\x1b[0m")
            .unwrap();
        let first = active_point(0);
        let last = point_with_x(first, COLS - 1);
        // Inspect the raw content tag: GridRef::style() is default for an RGB
        // background-only cell and was the reason the old test passed while
        // checking the wrong thing.
        for point in [first, last] {
            let cell = vt.grid_ref(point).unwrap().cell().unwrap();
            assert_eq!(cell.content_tag().unwrap(), CellContentTag::BgColorRgb);
            assert_eq!(cell.bg_color_rgb().unwrap(), RgbColor { r: 1, g: 2, b: 3 });
        }

        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        let mut replay = test_vt(DEFAULT_MAX_SCROLLBACK);
        replay.vt_write(&out);
        for point in [first, last] {
            assert_eq!(
                replay.grid_ref(point).unwrap().style().unwrap().bg_color,
                StyleColor::Rgb(RgbColor { r: 1, g: 2, b: 3 })
            );
        }
    }

    #[test]
    fn native_row_formatting_omits_terminal_metadata_for_blank_rows() {
        let vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut formatter = RowFormatter::new(COLS as usize);

        let row = formatter.format_row(&vt, active_point(0)).unwrap();

        assert!(
            row.len() < 32,
            "blank row output was unbounded: {} bytes",
            row.len()
        );
        assert!(
            !row.windows(2).any(|window| window == b"\x1b]"),
            "row formatter emitted OSC metadata: {row:?}"
        );
        assert!(
            !row.windows(4).any(|window| window == b"\x1b]4;"),
            "row formatter emitted palette entries: {row:?}"
        );
        assert!(
            row.is_empty(),
            "blank rows should rely on Clear(CurrentLine), got {row:?}"
        );
    }

    #[test]
    fn renderer_region_scroll_stays_in_sync() {
        // A DECSTBM region scroll moves rows without creating scrollback.
        // The dirty-tracking fast path must still pick up every moved row.
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        for i in 0..ROWS as usize {
            renderer.feed(&mut vt, format!("line{}\r\n", i)).unwrap();
        }
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        // Scroll rows 2-4 up by one inside a region, then reset the region.
        assert_eq!(
            renderer
                .feed(&mut vt, "\x1b[2;4r\x1b[4;1H\nnew\x1b[r")
                .unwrap(),
            0
        );
        renderer.render(&mut out, &vt).unwrap();

        assert_eq!(replayed_viewport(&out), viewport_lines(&vt));
    }

    #[test]
    fn renderer_height_shrink_without_pending_history_does_not_reflush() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer
            .feed(&mut vt, "line0\r\nline1\r\nline2\r\nline3\r\nold-bottom")
            .unwrap();
        renderer.render_full(&mut out, &vt).unwrap();
        out.clear();
        assert_eq!(renderer.unflushed(&vt).unwrap(), 0);

        let start = vt.track_grid_ref(active_point(0)).unwrap();
        vt.resize(COLS, ROWS - 1, 0, 0).unwrap();
        renderer.content_rows = ROWS - 1;
        let end = vt.track_grid_ref(active_point(0)).unwrap();
        renderer.push_resize_exclusion(start, end);
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

        renderer
            .feed(
                &mut vt,
                "abcdefghijABCDEFGHIJ\r\nline2\r\nline3\r\nline4\r\n",
            )
            .unwrap();
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();

        vt.resize(NEW_COLS, TEST_ROWS, 0, 0).unwrap();
        renderer.invalidate();
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        renderer
            .feed(&mut vt, "\r\nline5\r\nline6\r\nline7\r\nline8\r\n")
            .unwrap();
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
            renderer.feed(&mut vt, format!("line{}\r\n", i)).unwrap();
        }
        assert!(renderer.unflushed(&vt).unwrap() > 0);

        // Resize with unflushed rows pending: history reflows, the pins
        // follow, and the flush after resize emits the pending rows
        // instead of discarding them. Capture the exclusion interval just
        // as the session resize path does.
        let start = vt.track_grid_ref(active_point(0)).unwrap();
        vt.resize(COLS, ROWS - 1, 0, 0).unwrap();
        renderer.content_rows = ROWS - 1;
        let end = vt.track_grid_ref(active_point(0)).unwrap();
        renderer.push_resize_exclusion(start, end);
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

    #[test]
    fn renderer_resize_reflow_preserves_unflushed_scrollback() {
        const OLD_COLS: u16 = 10;
        const NEW_COLS: u16 = 4;
        const OLD_ROWS: u16 = 5;
        const NEW_ROWS: u16 = 4;

        let mut vt = test_vt_with_size(OLD_COLS, OLD_ROWS, DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer_with_rows(OLD_ROWS, &vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        // 8-wide lines fit at 10 cols but rewrap to two rows at 4 cols;
        // the endpoint pin must follow the reflow so the pending prefix
        // is still flushed in full.
        for i in 0..8 {
            renderer
                .feed(&mut vt, format!("A{i}B{i}C{i}D{i}\r\n"))
                .unwrap();
        }
        assert!(renderer.unflushed(&vt).unwrap() > 0);
        out.clear();

        // Shrink both axes, capturing the exclusion interval as the session
        // resize path does.
        let start = vt.track_grid_ref(active_point(0)).unwrap();
        vt.resize(NEW_COLS, NEW_ROWS, 0, 0).unwrap();
        renderer.content_rows = NEW_ROWS;
        let end = vt.track_grid_ref(active_point(0)).unwrap();
        renderer.push_resize_exclusion(start, end);
        renderer.invalidate();
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();

        let text = replayed_lines_with_size(&out, NEW_COLS, NEW_ROWS).join("");
        for i in 0..4 {
            let expected = format!("A{i}B{i}C{i}D{i}");
            assert_eq!(
                text.matches(&expected).count(),
                1,
                "pending line {i} lost or duplicated: {text:?}"
            );
        }
        for i in 4..7 {
            let expected = format!("A{i}B{i}C{i}D{i}");
            assert_eq!(
                text.matches(&expected).count(),
                0,
                "old viewport row {i} re-flushed: {text:?}"
            );
        }
    }

    #[test]
    fn renderer_consecutive_shrinks_exclude_each_viewport_interval() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        for i in 0..8 {
            renderer.feed(&mut vt, format!("line{i}\r\n")).unwrap();
        }
        assert!(renderer.unflushed(&vt).unwrap() > 0);
        out.clear();

        // First shrink queues an interval, but no flush runs before the
        // next resize (as when synchronized output is active).
        let start1 = vt.track_grid_ref(active_point(0)).unwrap();
        vt.resize(COLS, ROWS - 1, 0, 0).unwrap();
        renderer.content_rows = ROWS - 1;
        let end1 = vt.track_grid_ref(active_point(0)).unwrap();
        renderer.push_resize_exclusion(start1, end1);

        // Output between the shrinks scrolls a pending row below the first
        // interval; it must still reach native scrollback exactly once.
        renderer.feed(&mut vt, "mid0\r\n").unwrap();

        let start2 = vt.track_grid_ref(active_point(0)).unwrap();
        vt.resize(COLS, ROWS - 2, 0, 0).unwrap();
        renderer.content_rows = ROWS - 2;
        let end2 = vt.track_grid_ref(active_point(0)).unwrap();
        renderer.push_resize_exclusion(start2, end2);

        renderer.invalidate();
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        assert_eq!(renderer.unflushed(&vt).unwrap(), 0);
        assert!(renderer.resize_exclusions.is_empty());

        let lines = replayed_lines_with_size(&out, COLS, ROWS - 2);
        for expected in ["line0", "line1", "line2", "line3", "line5", "mid0"] {
            assert_eq!(
                lines.iter().filter(|l| **l == expected).count(),
                1,
                "pending row {expected} lost or duplicated: {lines:?}"
            );
        }
        for excluded in ["line4", "line6"] {
            assert_eq!(
                lines.iter().filter(|l| **l == excluded).count(),
                0,
                "old viewport row {excluded} re-flushed: {lines:?}"
            );
        }
    }

    #[test]
    fn renderer_reset_reestablishes_flush_boundary() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        for i in 0..8 {
            renderer.feed(&mut vt, format!("line{i}\r\n")).unwrap();
        }
        assert!(renderer.unflushed(&vt).unwrap() > 0);

        // RIS discards the grid and invalidates the boundary pin; feed must
        // repair it so accounting resumes from the surviving (empty) top.
        renderer.feed(&mut vt, "\x1bc").unwrap();
        assert!(renderer.flush_boundary.has_value());
        assert_eq!(renderer.unflushed(&vt).unwrap(), 0);

        // Output after the reset is pending again and flushes normally.
        for i in 0..6 {
            renderer.feed(&mut vt, format!("after{i}\r\n")).unwrap();
        }
        assert!(renderer.unflushed(&vt).unwrap() > 0);
        out.clear();
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        assert_eq!(renderer.unflushed(&vt).unwrap(), 0);

        let lines = replayed_lines(&out);
        assert_eq!(
            lines.iter().filter(|l| **l == "after0").count(),
            1,
            "post-reset row missing or duplicated: {lines:?}"
        );
        assert_eq!(
            lines.iter().filter(|l| **l == "line0").count(),
            0,
            "pre-reset row re-emitted: {lines:?}"
        );
    }

    #[test]
    fn renderer_reset_clears_pending_resize_exclusions() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        for i in 0..8 {
            renderer.feed(&mut vt, format!("line{i}\r\n")).unwrap();
        }
        let start = vt.track_grid_ref(active_point(0)).unwrap();
        vt.resize(COLS, ROWS - 1, 0, 0).unwrap();
        renderer.content_rows = ROWS - 1;
        let end = vt.track_grid_ref(active_point(0)).unwrap();
        renderer.push_resize_exclusion(start, end);

        // A reset invalidates the pending interval; the reconciliation must
        // drop it so no later flush resolves dead pins.
        renderer.feed(&mut vt, "\x1bc").unwrap();
        assert!(renderer.resize_exclusions.is_empty());
        assert_eq!(renderer.unflushed(&vt).unwrap(), 0);

        renderer.feed(&mut vt, "after\r\n").unwrap();
        renderer.invalidate();
        out.clear();
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        let lines = replayed_lines_with_size(&out, COLS, ROWS - 1);
        assert_eq!(
            lines.iter().filter(|l| **l == "after").count(),
            1,
            "post-reset row missing or duplicated: {lines:?}"
        );
    }

    #[test]
    fn renderer_reset_split_across_feed_buffers() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        for i in 0..8 {
            renderer.feed(&mut vt, format!("line{i}\r\n")).unwrap();
        }
        assert!(renderer.unflushed(&vt).unwrap() > 0);

        // ESC and c arrive in separate buffers; the scanner state must
        // carry across so the second feed is recognized as RIS.
        renderer.feed(&mut vt, "\x1b").unwrap();
        renderer.feed(&mut vt, "cafter0\r\n").unwrap();
        assert!(renderer.flush_boundary.has_value());
        assert_eq!(renderer.unflushed(&vt).unwrap(), 0);

        out.clear();
        renderer
            .feed(&mut vt, "after1\r\nafter2\r\nafter3\r\nafter4\r\n")
            .unwrap();
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        let lines = replayed_lines(&out);
        assert_eq!(
            lines.iter().filter(|l| **l == "after0").count(),
            1,
            "post-reset row missing or duplicated: {lines:?}"
        );
        assert_eq!(
            lines.iter().filter(|l| **l == "line0").count(),
            0,
            "pre-reset row re-emitted: {lines:?}"
        );
    }

    #[test]
    fn renderer_reset_followed_by_output_in_same_buffer() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        for i in 0..8 {
            renderer.feed(&mut vt, format!("line{i}\r\n")).unwrap();
        }
        assert!(renderer.unflushed(&vt).unwrap() > 0);
        out.clear();

        // The same chunk resets and then scrolls new content into history.
        // Everything after the RIS is unflushed and must be emitted exactly
        // once; scrollback being non-empty must not mask the reset.
        let mut chunk = String::from("\x1bc");
        for i in 0..8 {
            chunk.push_str(&format!("after{i}\r\n"));
        }
        renderer.feed(&mut vt, chunk).unwrap();
        assert!(renderer.flush_boundary.has_value());
        assert_eq!(renderer.unflushed(&vt).unwrap(), 4);

        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        assert_eq!(renderer.unflushed(&vt).unwrap(), 0);
        let lines = replayed_lines(&out);
        for i in 0..4 {
            let expected = format!("after{i}");
            assert_eq!(
                lines.iter().filter(|l| **l == expected).count(),
                1,
                "post-reset row {expected} lost or duplicated: {lines:?}"
            );
        }
        assert_eq!(
            lines.iter().filter(|l| **l == "line0").count(),
            0,
            "pre-reset row re-emitted: {lines:?}"
        );
    }

    #[test]
    fn renderer_reset_defers_reconcile_while_alternate_screen_active() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        for i in 0..8 {
            renderer.feed(&mut vt, format!("line{i}\r\n")).unwrap();
        }
        assert!(renderer.unflushed(&vt).unwrap() > 0);

        // RIS lands on the primary screen, but the same chunk immediately
        // enters the alternate screen: the primary pin must not be
        // re-anchored while the alternate page list is active.
        renderer.feed(&mut vt, "\x1bc\x1b[?1049h").unwrap();
        assert!(!renderer.flush_boundary.has_value());
        assert_eq!(renderer.unflushed(&vt).unwrap(), 0);
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();

        // RIS while the alternate screen is in use exits it. Reconciliation
        // now runs on the primary screen and repairs the invalid boundary.
        renderer.feed(&mut vt, "alt content\x1bc").unwrap();
        assert!(renderer.flush_boundary.has_value());
        assert_eq!(renderer.unflushed(&vt).unwrap(), 0);

        out.clear();
        for i in 0..6 {
            renderer.feed(&mut vt, format!("after{i}\r\n")).unwrap();
        }
        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        let lines = replayed_lines(&out);
        assert_eq!(
            lines.iter().filter(|l| **l == "after0").count(),
            1,
            "post-reset row missing or duplicated: {lines:?}"
        );
        assert_eq!(
            lines.iter().filter(|l| **l == "line0").count(),
            0,
            "pre-reset row re-emitted: {lines:?}"
        );
    }

    #[test]
    fn renderer_pruning_drops_fully_pruned_exclusion() {
        // Tiny scrollback budget so pruning consumes the pending prefix and
        // the queued exclusion while it waits for a flush.
        let mut vt = test_vt(2_000);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        for i in 0..8 {
            renderer.feed(&mut vt, format!("line{i}\r\n")).unwrap();
        }
        let start = vt.track_grid_ref(active_point(0)).unwrap();
        vt.resize(COLS, ROWS - 1, 0, 0).unwrap();
        renderer.content_rows = ROWS - 1;
        let end = vt.track_grid_ref(active_point(0)).unwrap();
        renderer.push_resize_exclusion(start, end);

        // Sustained output eventually evicts the page holding the interval
        // pins. Reconciliation repairs each casualty exactly: the boundary
        // resumes at the surviving top, a half-pruned interval resumes at
        // its surviving prefix, and a fully pruned one is dropped.
        let mut fed = 0;
        while !renderer.resize_exclusions.is_empty() && fed < 20_000 {
            renderer.feed(&mut vt, format!("bulk{fed}\r\n")).unwrap();
            fed += 1;
        }
        assert!(
            renderer.resize_exclusions.is_empty(),
            "pruning never consumed the exclusion after {fed} rows"
        );

        renderer.render_with_scroll(&mut out, &mut vt).unwrap();
        assert_eq!(renderer.unflushed(&vt).unwrap(), 0);

        let lines = replayed_lines_with_size(&out, COLS, ROWS - 1);
        for i in fed - 10..fed {
            let expected = format!("bulk{i}");
            assert_eq!(
                lines.iter().filter(|l| **l == expected).count(),
                1,
                "recent row {expected} lost or duplicated after pruning"
            );
        }
    }

    #[test]
    fn renderer_unexpected_pin_loss_fails_before_output() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut out = Vec::new();
        renderer.render_full(&mut out, &vt).unwrap();

        for i in 0..8 {
            renderer.feed(&mut vt, format!("line{i}\r\n")).unwrap();
        }

        // Mutate the VT behind the renderer's back so no reconciliation
        // runs: the dead boundary must fail rendering closed, before any
        // bytes are emitted, instead of guessing a flush region.
        vt.vt_write("\x1bc".as_bytes());
        out.clear();
        let err = renderer.render_with_scroll(&mut out, &mut vt);
        assert!(err.is_err(), "render must fail on unexpected pin loss");
        assert!(out.is_empty(), "no output may be emitted on failure");
    }

    fn filter_chunks(chunks: &[&[u8]]) -> Vec<u8> {
        let mut filter = VtInputFilter::new();
        let mut output = Vec::new();
        let mut combined = Vec::new();

        for chunk in chunks {
            let filtered = filter.filter(chunk, &mut output);
            combined.extend_from_slice(filtered.as_bytes());
        }

        combined
    }

    #[test]
    fn vt_input_filter_borrows_ordinary_ground_output() {
        let mut filter = VtInputFilter::new();
        let mut output = Vec::new();
        let input = b"ordinary PTY output";

        let filtered = filter.filter(input, &mut output);
        let FilteredVtInput::Borrowed(bytes) = filtered else {
            panic!("ordinary ground output was unnecessarily copied");
        };
        assert_eq!(bytes.as_ptr(), input.as_ptr());
        assert_eq!(bytes, input);
        assert!(output.is_empty());
    }

    #[test]
    fn raw_pty_bytes_preserve_invalid_utf8_for_ghostty() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut filter = VtInputFilter::new();
        let mut output = Vec::new();

        let filtered = filter.filter(b"a\xffb", &mut output);
        renderer.feed(&mut vt, filtered.as_bytes()).unwrap();

        // Ghostty's byte-stream parser, rather than a pre-parser String
        // conversion, applies the terminal-standard replacement behavior.
        assert_eq!(viewport_lines(&vt)[0], "a\u{fffd}b");
    }

    #[test]
    fn raw_pty_bytes_preserve_split_utf8_for_ghostty() {
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        let mut renderer = test_renderer(&vt);
        let mut filter = VtInputFilter::new();
        let mut output = Vec::new();

        let first = filter.filter(b"\xe2\x82", &mut output);
        renderer.feed(&mut vt, first.as_bytes()).unwrap();
        assert_eq!(viewport_lines(&vt)[0], "");

        let second = filter.filter(b"\xac", &mut output);
        renderer.feed(&mut vt, second.as_bytes()).unwrap();
        assert_eq!(viewport_lines(&vt)[0], "€");
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

    #[test]
    fn virtual_pty_reply_filter_is_exact_and_deny_by_default() {
        let cases = [
            (b"\x1b[0n".as_slice(), true),
            (b"\x1b[12;80R", true),
            (b"\x1b[?2048;0$y", true),
            (b"\x1b[?2048;1$y", true),
            (b"\x1b[?2048;2$y", true),
            (b"\x1b[?2048;3$y", true),
            (b"\x1b[?2048;4$y", true),
            // Responses from physical-owned queries must never reach the
            // child through the virtual hook.
            (b"\x1b[?7;1$y", false),
            (b"\x1b[?2048;5$y", false),
            (b"\x1b]10;rgb:0000/0000/0000\x1b\\", false),
            (b"\x1bP>|libghostty\x1b\\", false),
            (b"\x1b[48;23;80;0;0t", false),
            (b"\x1b[;80R", false),
        ];

        for (reply, expected) in cases {
            assert_eq!(
                is_virtual_terminal_reply(reply),
                expected,
                "incorrect ownership for reply {reply:?}",
            );
        }
    }

    #[test]
    fn virtual_query_hook_handles_mixed_and_split_queries() {
        let replies = RefCell::new(VirtualPtyReplies::default());
        let mut vt = test_vt(DEFAULT_MAX_SCROLLBACK);
        vt.on_pty_write({
            let replies = &replies;
            move |_term, data| replies.borrow_mut().capture(data)
        })
        .unwrap();

        // The CPR request and mode query are split at arbitrary byte
        // boundaries. `?7$p` and XTVERSION still invoke libghostty-vt's
        // general callback, but the collector deliberately discards them so
        // their established physical passthrough remains the sole response.
        vt.vt_write(b"\x1b[3;4H\x1b[5n\x1b[");
        vt.vt_write(b"6n\x1b[?2048$");
        vt.vt_write(b"p\x1b[?7$p\x1b[>q");

        assert_eq!(replies.borrow().bytes, b"\x1b[0n\x1b[3;4R\x1b[?2048;2$y");

        // The virtual terminal's mode changes from the raw PTY stream, even
        // though the corresponding mode transition is never sent to stdout.
        replies.borrow_mut().bytes.clear();
        vt.vt_write(b"\x1b[?2048h\x1b[?2048$p");
        assert_eq!(replies.borrow().bytes, b"\x1b[?2048;1$y");

        // Enabling the callback also makes Ghostty emit an in-band resize
        // report from `resize`. The session sends its own PTY-sized report,
        // so the generic hook must continue rejecting this duplicate.
        replies.borrow_mut().bytes.clear();
        vt.resize(COLS + 1, ROWS, 0, 0).unwrap();
        assert!(replies.borrow().bytes.is_empty());
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
