//! Shared utilities for reading cell text from a libghostty-rs terminal.

use libghostty_vt::screen::{Cell, CellContentTag, CellWide, GridRef};
use libghostty_vt::terminal::{Point, PointCoordinate, Terminal};

/// Default scrollback limit used across devenv shell and reload.
pub const DEFAULT_MAX_SCROLLBACK: usize = 10_000;

/// Return a copy of `point` with the x coordinate changed.
pub fn point_with_x(point: Point, x: u16) -> Point {
    match point {
        Point::Active(c) => Point::Active(PointCoordinate { x, ..c }),
        Point::Viewport(c) => Point::Viewport(PointCoordinate { x, ..c }),
        Point::Screen(c) => Point::Screen(PointCoordinate { x, ..c }),
        Point::History(c) => Point::History(PointCoordinate { x, ..c }),
    }
}

/// Construct an Active coordinate point with x=0.
pub fn active_point(y: u32) -> Point {
    Point::Active(PointCoordinate { x: 0, y })
}

/// Construct a Screen coordinate point with x=0.
pub fn screen_point(y: u32) -> Point {
    Point::Screen(PointCoordinate { x: 0, y })
}

/// Get all cells in a row by iterating columns via grid ref.
pub fn cells_in_row(vt: &Terminal<'_, '_>, point: Point) -> Vec<Cell> {
    let cols = vt.cols().unwrap_or(0);
    (0..cols)
        .filter_map(|x| vt.grid_ref(point_with_x(point, x)).ok()?.cell().ok())
        .collect()
}

/// Push a cell's text content into `buf`, using the grid ref for grapheme clusters.
///
/// Skips spacer-tail cells. Pushes a space for empty cells.
pub fn push_cell_text(buf: &mut String, cell: &Cell, cell_ref: &GridRef<'_>) {
    if cell.wide().ok() == Some(CellWide::SpacerTail) {
        return;
    }
    if cell.has_text().unwrap_or(false) {
        if cell.content_tag().ok() == Some(CellContentTag::CodepointGrapheme) {
            let mut grapheme_buf = ['\0'; 32];
            if let Ok(len) = cell_ref.graphemes(&mut grapheme_buf) {
                for ch in &grapheme_buf[..len] {
                    buf.push(*ch);
                }
            }
        } else if let Some(ch) = cell.codepoint().ok().and_then(char::from_u32) {
            buf.push(ch);
        }
    } else {
        buf.push(' ');
    }
}

/// Cursor position and visibility snapshot from a terminal.
#[derive(Clone, Copy, PartialEq)]
pub struct CursorState {
    pub col: u16,
    pub row: u16,
    pub visible: bool,
}

impl CursorState {
    pub fn from_terminal(vt: &Terminal<'_, '_>) -> Self {
        Self {
            col: vt.cursor_x().unwrap_or(0),
            row: vt.cursor_y().unwrap_or(0),
            visible: vt.is_cursor_visible().unwrap_or(true),
        }
    }
}

/// Extract plain text (no SGR styling) from a single terminal row.
pub fn row_plain_text(vt: &Terminal<'_, '_>, point: Point) -> String {
    let cols = vt.cols().unwrap_or(0);
    let mut text = String::new();
    for x in 0..cols {
        let Ok(gr) = vt.grid_ref(point_with_x(point, x)) else {
            continue;
        };
        let Ok(cell) = gr.cell() else {
            continue;
        };
        push_cell_text(&mut text, &cell, &gr);
    }
    text
}
