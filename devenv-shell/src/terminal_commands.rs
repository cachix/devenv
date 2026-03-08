//! Terminal escape sequence commands not covered by crossterm.
//!
//! Each struct implements crossterm's `Command` trait so it can be used
//! with `queue!` / `execute!` alongside built-in crossterm commands.

use crossterm::Command;
use std::fmt;

/// XTWINOPS response: report text area size in characters (CSI 8 ; rows ; cols t).
pub struct ReportTextAreaSize {
    pub rows: u16,
    pub cols: u16,
}

impl Command for ReportTextAreaSize {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[8;{};{}t", self.rows, self.cols)
    }
}

/// DECSTBM — set top/bottom scroll region (CSI top ; bottom r).
pub struct SetScrollRegion {
    pub top: u16,
    pub bottom: u16,
}

impl Command for SetScrollRegion {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[{};{}r", self.top, self.bottom)
    }
}

/// Reset scroll region to full screen (CSI r).
pub struct ResetScrollRegion;

impl Command for ResetScrollRegion {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        f.write_str("\x1b[r")
    }
}

/// Reset a DEC private mode (CSI ? mode l).
pub struct ResetDecMode(pub u16);

/// DEC origin mode (mode 6).
pub const ORIGIN_MODE: u16 = 6;

impl Command for ResetDecMode {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[?{}l", self.0)
    }
}

/// DSR — device status report / cursor position query (CSI 6 n).
pub struct CursorPositionQuery;

impl Command for CursorPositionQuery {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        f.write_str("\x1b[6n")
    }
}

/// DECKPAM (ESC =) / DECKPNM (ESC >).
pub struct SetKeypadMode {
    pub application: bool,
}

impl Command for SetKeypadMode {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        f.write_str(if self.application { "\x1b=" } else { "\x1b>" })
    }
}

/// XTMODIFYOTHERKEYS reset (CSI > n).
pub struct ResetModifyOtherKeys;

impl Command for ResetModifyOtherKeys {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        f.write_str("\x1b[>n")
    }
}
