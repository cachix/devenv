//! Escape-sequence state tracking for forwarded terminal modes.
//!
//! Wraps `EscapeScanner` output to keep a running record of modes the embedded
//! shell has set on the user's terminal (alt-screen, mouse, bracketed paste,
//! kitty keyboard, etc.) so the host can reset them when the shell exits or
//! when a hot-reload swaps PTYs.

use crate::escape::{CLEANUP_MODES, DecModeEvent, EscapeScanner, SequenceEvent};
use crate::pty::Pty;
use crate::terminal_commands::{
    ReportTextAreaSize, ResetDecMode, ResetModifyOtherKeys, SetKeypadMode,
};
use crossterm::event::PopKeyboardEnhancementFlags;
use crossterm::{Command, queue, terminal};
use portable_pty::PtySize;
use std::collections::BTreeSet;
use std::io::{self, Write};

/// Escape-sequence state tracked across PTY output processing.
///
/// Persistent fields (`in_alternate_screen`, `forwarded_dec_modes`,
/// `keypad_application_mode`) carry across the entire session.
/// Per-batch fields (`erase_display`, `clear_scrollback`) are reset at the
/// start of each `PtyOutput` batch.
pub struct EscapeState {
    pub in_alternate_screen: bool,
    /// DEC private modes that were set and need explicit reset on exit.
    /// Tracked separately from `in_alternate_screen` (which uses
    /// `LeaveAlternateScreen`) and keypad mode (which uses `ESC >`).
    pub forwarded_dec_modes: BTreeSet<u16>,
    /// Keypad is in application mode (DECKPAM, `ESC =`).
    pub keypad_application_mode: bool,
    /// Set when CSI 2 J is seen — signals the caller to consume `row_offset`.
    pub erase_display: bool,
    /// Set when CSI 3 J is seen — deferred so the caller can emit it *after*
    /// `scroll_region` pushes old TUI content into scrollback.
    pub clear_scrollback: bool,
    /// Kitty keyboard protocol stack depth.
    pub kitty_keyboard_depth: u32,
    /// XTMODIFYOTHERKEYS is enabled.
    pub modify_other_keys: bool,
    /// Mode 2048 (in-band resize) is enabled by the PTY program.
    pub in_band_resize: bool,
}

impl EscapeState {
    pub fn new() -> Self {
        Self {
            in_alternate_screen: false,
            forwarded_dec_modes: BTreeSet::new(),
            keypad_application_mode: false,
            erase_display: false,
            clear_scrollback: false,
            kitty_keyboard_depth: 0,
            modify_other_keys: false,
            in_band_resize: false,
        }
    }

    /// Reset per-batch flags before processing a new `PtyOutput` batch.
    pub fn reset_batch(&mut self) {
        self.erase_display = false;
        self.clear_scrollback = false;
    }

    /// Apply a DEC mode event, updating tracked state.
    /// Returns the raw bytes to forward to stdout (empty if mode isn't forwarded).
    pub fn apply_dec_mode<'a>(&mut self, event: &'a DecModeEvent) -> &'a [u8] {
        if event.enables_in_band_resize() {
            self.in_band_resize = true;
        } else if event.disables_in_band_resize() {
            self.in_band_resize = false;
        }

        if !event.has_forwarded_mode() {
            return &[];
        }

        if event.enters_alt_screen() {
            self.in_alternate_screen = true;
        } else if event.exits_alt_screen() {
            self.in_alternate_screen = false;
        }

        match event {
            DecModeEvent::Set { modes, .. } => {
                for &m in modes {
                    if CLEANUP_MODES.contains(&m) {
                        self.forwarded_dec_modes.insert(m);
                    }
                }
            }
            DecModeEvent::Reset { modes, .. } => {
                for m in modes {
                    self.forwarded_dec_modes.remove(m);
                }
            }
        }

        event.raw_bytes()
    }

    /// Apply a kitty keyboard push/pop/set event.
    pub fn apply_kitty_keyboard(&mut self, stack_delta: i8) {
        if stack_delta > 0 {
            self.kitty_keyboard_depth =
                self.kitty_keyboard_depth.saturating_add(stack_delta as u32);
        } else if stack_delta < 0 {
            self.kitty_keyboard_depth = self
                .kitty_keyboard_depth
                .saturating_sub(stack_delta.unsigned_abs() as u32);
        }
    }

    /// Apply an XTMODIFYOTHERKEYS set/reset event.
    pub fn apply_modify_other_keys(&mut self, enabled: bool) {
        self.modify_other_keys = enabled;
    }
}

impl Default for EscapeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Scan raw PTY output for escape sequences (DEC private mode and OSC queries),
/// forward relevant ones to the real terminal, and update escape state.
///
/// Pass `&mut std::io::sink()` for `stdout` to track state without forwarding
/// any bytes (useful when the caller writes raw PTY bytes themselves and only
/// needs the tracking side-effect).
pub fn process_escape_events(
    scanner: &mut EscapeScanner,
    data: &[u8],
    esc: &mut EscapeState,
    stdout: &mut impl Write,
    pty: &Pty,
    pty_size: PtySize,
    events_buf: &mut Vec<SequenceEvent>,
) -> io::Result<()> {
    events_buf.clear();
    scanner.scan_into(data, events_buf);
    for event in events_buf.drain(..) {
        match event {
            SequenceEvent::DecMode(event) => {
                let forward = esc.apply_dec_mode(&event);
                if !forward.is_empty() {
                    stdout.write_all(forward)?;
                }
            }
            SequenceEvent::Osc(event) => {
                stdout.write_all(&event.raw_bytes)?;
            }
            SequenceEvent::EraseDisplay { .. } => {
                esc.erase_display = true;
            }
            SequenceEvent::ClearScrollback { .. } => {
                esc.clear_scrollback = true;
            }
            SequenceEvent::ForwardCsi { raw_bytes } => {
                stdout.write_all(&raw_bytes)?;
            }
            SequenceEvent::ForwardDcs { raw_bytes } => {
                stdout.write_all(&raw_bytes)?;
            }
            SequenceEvent::KittyKeyboard {
                raw_bytes,
                stack_delta,
            } => {
                stdout.write_all(&raw_bytes)?;
                esc.apply_kitty_keyboard(stack_delta);
            }
            SequenceEvent::ModifyOtherKeys { raw_bytes, enabled } => {
                stdout.write_all(&raw_bytes)?;
                esc.apply_modify_other_keys(enabled);
            }
            SequenceEvent::TextAreaSizeQuery => {
                let cmd = ReportTextAreaSize {
                    rows: pty_size.rows,
                    cols: pty_size.cols,
                };
                let mut buf = String::new();
                cmd.write_ansi(&mut buf).unwrap();
                pty.write_all(buf.as_bytes())?;
                pty.flush()?;
            }
            SequenceEvent::KeypadMode { application } => {
                queue!(stdout, SetKeypadMode { application })?;
                esc.keypad_application_mode = application;
            }
        }
    }
    Ok(())
}

/// Reset any forwarded DEC modes on exit so the terminal is left clean.
///
/// XTSHIFTESCAPE and DECSCUSR are forwarded without explicit cleanup:
/// mouse tracking modes (cleaned up above) make XTSHIFTESCAPE inert,
/// and most terminals reset cursor shape on their own.
pub fn cleanup_forwarded_modes(esc: &EscapeState, stdout: &mut impl Write) -> io::Result<()> {
    let mut needs_flush = false;
    if esc.in_alternate_screen {
        queue!(stdout, terminal::LeaveAlternateScreen)?;
        needs_flush = true;
    }
    for &mode in &esc.forwarded_dec_modes {
        queue!(stdout, ResetDecMode(mode))?;
        needs_flush = true;
    }
    if esc.keypad_application_mode {
        queue!(stdout, SetKeypadMode { application: false })?;
        needs_flush = true;
    }
    for _ in 0..esc.kitty_keyboard_depth {
        queue!(stdout, PopKeyboardEnhancementFlags)?;
        needs_flush = true;
    }
    if esc.modify_other_keys {
        queue!(stdout, ResetModifyOtherKeys)?;
        needs_flush = true;
    }
    if needs_flush {
        stdout.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed raw bytes through scanner and apply events to state.
    /// Returns bytes that would be forwarded to stdout.
    fn scan_and_apply(scanner: &mut EscapeScanner, esc: &mut EscapeState, input: &[u8]) -> Vec<u8> {
        let events = scanner.scan(input);
        let mut forwarded = Vec::new();
        for event in &events {
            match event {
                SequenceEvent::DecMode(ev) => forwarded.extend_from_slice(esc.apply_dec_mode(ev)),
                SequenceEvent::KittyKeyboard { stack_delta, .. } => {
                    esc.apply_kitty_keyboard(*stack_delta);
                }
                SequenceEvent::ModifyOtherKeys { enabled, .. } => {
                    esc.apply_modify_other_keys(*enabled);
                }
                _ => {}
            }
        }
        forwarded
    }

    #[test]
    fn in_band_resize_tracked_on_opt_in() {
        let mut scanner = EscapeScanner::new();
        let mut esc = EscapeState::new();
        let forwarded = scan_and_apply(&mut scanner, &mut esc, b"\x1b[?2048h");
        assert!(esc.in_band_resize);
        assert!(forwarded.is_empty(), "mode 2048 should not be forwarded");
    }

    #[test]
    fn in_band_resize_cleared_on_opt_out() {
        let mut scanner = EscapeScanner::new();
        let mut esc = EscapeState::new();
        scan_and_apply(&mut scanner, &mut esc, b"\x1b[?2048h");
        assert!(esc.in_band_resize);
        scan_and_apply(&mut scanner, &mut esc, b"\x1b[?2048l");
        assert!(!esc.in_band_resize);
    }

    #[test]
    fn in_band_resize_compound_mode() {
        let mut scanner = EscapeScanner::new();
        let mut esc = EscapeState::new();
        let forwarded = scan_and_apply(&mut scanner, &mut esc, b"\x1b[?1;2048h");
        assert!(esc.in_band_resize);
        assert!(!forwarded.is_empty(), "mode 1 should be forwarded");
    }

    #[test]
    fn alt_screen_enter_exit() {
        let mut scanner = EscapeScanner::new();
        let mut esc = EscapeState::new();
        scan_and_apply(&mut scanner, &mut esc, b"\x1b[?1049h");
        assert!(esc.in_alternate_screen);

        scan_and_apply(&mut scanner, &mut esc, b"\x1b[?1049l");
        assert!(!esc.in_alternate_screen);
    }

    #[test]
    fn alt_screen_all_variants() {
        for mode in [47, 1047, 1049] {
            let mut scanner = EscapeScanner::new();
            let mut esc = EscapeState::new();
            let seq = format!("\x1b[?{}h", mode);
            scan_and_apply(&mut scanner, &mut esc, seq.as_bytes());
            assert!(
                esc.in_alternate_screen,
                "mode {} should enter alt screen",
                mode
            );
        }
    }

    #[test]
    fn forwarded_modes_accumulate() {
        let mut scanner = EscapeScanner::new();
        let mut esc = EscapeState::new();
        scan_and_apply(&mut scanner, &mut esc, b"\x1b[?1000h");
        scan_and_apply(&mut scanner, &mut esc, b"\x1b[?2004h");
        assert!(esc.forwarded_dec_modes.contains(&1000));
        assert!(esc.forwarded_dec_modes.contains(&2004));
    }

    #[test]
    fn non_forwarded_mode_no_stdout() {
        let mut scanner = EscapeScanner::new();
        let mut esc = EscapeState::new();
        let forwarded = scan_and_apply(&mut scanner, &mut esc, b"\x1b[?2048h");
        assert!(forwarded.is_empty());
    }

    #[test]
    fn kitty_keyboard_depth_tracking() {
        let mut scanner = EscapeScanner::new();
        let mut esc = EscapeState::new();
        scan_and_apply(&mut scanner, &mut esc, b"\x1b[>1u");
        assert_eq!(esc.kitty_keyboard_depth, 1);
        scan_and_apply(&mut scanner, &mut esc, b"\x1b[>1u");
        assert_eq!(esc.kitty_keyboard_depth, 2);
        scan_and_apply(&mut scanner, &mut esc, b"\x1b[<u");
        assert_eq!(esc.kitty_keyboard_depth, 1);
    }

    #[test]
    fn modify_other_keys_tracking() {
        let mut scanner = EscapeScanner::new();
        let mut esc = EscapeState::new();
        scan_and_apply(&mut scanner, &mut esc, b"\x1b[>4m");
        assert!(esc.modify_other_keys);
        scan_and_apply(&mut scanner, &mut esc, b"\x1b[>0m");
        assert!(!esc.modify_other_keys);
    }

    #[test]
    fn cleanup_resets_tracked_modes() {
        let mut scanner = EscapeScanner::new();
        let mut esc = EscapeState::new();
        scan_and_apply(&mut scanner, &mut esc, b"\x1b[?1049h\x1b[?1000h\x1b[?2004h");
        assert!(esc.in_alternate_screen);
        assert!(esc.forwarded_dec_modes.contains(&1000));
        assert!(esc.forwarded_dec_modes.contains(&2004));

        let mut buf: Vec<u8> = Vec::new();
        cleanup_forwarded_modes(&esc, &mut buf).expect("cleanup should not fail");
        let out = String::from_utf8_lossy(&buf);
        assert!(
            out.contains("\x1b[?1049l"),
            "expected alt-screen reset, got {:?}",
            out
        );
        assert!(
            out.contains("\x1b[?1000l"),
            "expected mouse reset, got {:?}",
            out
        );
        assert!(
            out.contains("\x1b[?2004l"),
            "expected paste reset, got {:?}",
            out
        );
    }
}
