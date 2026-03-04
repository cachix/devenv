//! Stateful byte-level scanner for escape sequences in raw PTY output.
//!
//! Detects DEC private mode sequences (`CSI ? <params> h/l`) and
//! OSC queries (`OSC ... ? BEL/ST`) so they can be forwarded to
//! the real terminal (avt consumes them internally).

mod dec_mode;
mod osc;

use dec_mode::{DecModeAction, DecModeParser, DecModeResult};
use osc::{OscParser, OscResult};

/// DEC private modes that should be forwarded to the real terminal.
const FORWARDED_MODES: &[u16] = &[
    1, // cursor key mode (DECCKM) — applications use smkx/rmkx to toggle
    47, 1047, 1049, // alternate screen
    1000, 1002, 1003, // mouse tracking
    1005, 1006, 1015, // mouse encoding
    2004, // bracketed paste
    1004, // focus events
    2026, // synchronized output
];

/// Modes that control alternate screen buffer.
const ALT_SCREEN_MODES: &[u16] = &[47, 1047, 1049];

/// Modes that need explicit reset on session exit. Excludes alternate screen
/// modes (handled via `LeaveAlternateScreen`) and synchronized output (mode
/// 2026, managed per-frame by the renderer).
pub const CLEANUP_MODES: &[u16] = &[
    1, // cursor key mode (DECCKM)
    1000, 1002, 1003, // mouse tracking
    1005, 1006, 1015, // mouse encoding
    2004, // bracketed paste
    1004, // focus events
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecModeEvent {
    /// `CSI ? <modes> h` — set (enable) modes.
    Set { modes: Vec<u16>, raw_bytes: Vec<u8> },
    /// `CSI ? <modes> l` — reset (disable) modes.
    Reset { modes: Vec<u16>, raw_bytes: Vec<u8> },
}

impl DecModeEvent {
    /// Whether any mode in this event should be forwarded to the real terminal.
    pub fn has_forwarded_mode(&self) -> bool {
        let modes = match self {
            DecModeEvent::Set { modes, .. } | DecModeEvent::Reset { modes, .. } => modes,
        };
        modes.iter().any(|m| FORWARDED_MODES.contains(m))
    }

    /// Whether this event enters alternate screen.
    pub fn enters_alt_screen(&self) -> bool {
        matches!(self, DecModeEvent::Set { modes, .. } if modes.iter().any(|m| ALT_SCREEN_MODES.contains(m)))
    }

    /// Whether this event exits alternate screen.
    pub fn exits_alt_screen(&self) -> bool {
        matches!(self, DecModeEvent::Reset { modes, .. } if modes.iter().any(|m| ALT_SCREEN_MODES.contains(m)))
    }

    /// Raw bytes of the original sequence for forwarding.
    pub fn raw_bytes(&self) -> &[u8] {
        match self {
            DecModeEvent::Set { raw_bytes, .. } | DecModeEvent::Reset { raw_bytes, .. } => {
                raw_bytes
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OscEvent {
    pub raw_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceEvent {
    DecMode(DecModeEvent),
    Osc(OscEvent),
    /// CSI 2 J — erase entire display.
    EraseDisplay {
        raw_bytes: Vec<u8>,
    },
    /// CSI 3 J — erase scrollback buffer.
    ClearScrollback {
        raw_bytes: Vec<u8>,
    },
    /// CSI c or CSI 0 c — Primary Device Attributes request.
    /// Programs send this to discover terminal type; crossterm also uses it
    /// as a sync marker to terminate pending capability queries.
    PrimaryDA {
        raw_bytes: Vec<u8>,
    },
    /// `ESC =` (DECKPAM) or `ESC >` (DECKPNM) — keypad application/numeric mode.
    /// Part of `smkx`/`rmkx` terminfo capabilities alongside DECCKM.
    KeypadMode {
        /// `true` = application mode (DECKPAM, `ESC =`),
        /// `false` = numeric mode (DECKPNM, `ESC >`).
        application: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RouterState {
    Ground,
    Esc,
    Csi,
    /// Standard (non-DEC-private) CSI sequence: `CSI <digits> <final>`.
    CsiStandard,
    Osc,
    /// ESC seen while in OSC — could be ST (`ESC \`) or a new sequence.
    OscEsc,
}

/// Byte-level scanner that detects escape sequences in raw PTY output.
///
/// Routes `ESC [` to the DEC private mode parser and `ESC ]` to the
/// OSC parser.
/// State persists across calls to handle sequences split across buffer boundaries.
pub struct EscapeScanner {
    state: RouterState,
    seq_bytes: Vec<u8>,
    dec_parser: DecModeParser,
    osc_parser: OscParser,
    /// Accumulated numeric parameter for standard CSI sequences.
    csi_param: u16,
}

impl EscapeScanner {
    pub fn new() -> Self {
        Self {
            state: RouterState::Ground,
            seq_bytes: Vec::new(),
            dec_parser: DecModeParser::new(),
            osc_parser: OscParser::new(),
            csi_param: 0,
        }
    }

    /// Scan a chunk of raw PTY output and return any escape sequence events found.
    ///
    /// Convenience wrapper around [`scan_into`](Self::scan_into) that allocates
    /// a new Vec per call. Prefer `scan_into` on hot paths.
    #[cfg(test)]
    pub fn scan(&mut self, data: &[u8]) -> Vec<SequenceEvent> {
        let mut events = Vec::new();
        self.scan_into(data, &mut events);
        events
    }

    /// Scan a chunk of raw PTY output, appending events to the provided Vec.
    ///
    /// Unlike [`scan`], this avoids allocating a new Vec on every call.
    /// The caller can reuse the Vec across invocations.
    pub fn scan_into(&mut self, data: &[u8], events: &mut Vec<SequenceEvent>) {
        for &byte in data {
            match self.state {
                RouterState::Ground => {
                    if byte == 0x1b {
                        self.state = RouterState::Esc;
                        self.seq_bytes.clear();
                        self.seq_bytes.push(byte);
                    }
                }

                RouterState::Esc => {
                    self.seq_bytes.push(byte);
                    match byte {
                        b'[' => {
                            self.state = RouterState::Csi;
                            self.dec_parser.reset();
                        }
                        b']' => {
                            self.state = RouterState::Osc;
                            self.osc_parser.reset();
                        }
                        b'=' | b'>' => {
                            // DECKPAM (ESC =) / DECKPNM (ESC >)
                            self.seq_bytes.clear();
                            events.push(SequenceEvent::KeypadMode {
                                application: byte == b'=',
                            });
                            self.state = RouterState::Ground;
                        }
                        0x1b => {
                            // Another ESC restarts the sequence
                            self.seq_bytes.clear();
                            self.seq_bytes.push(byte);
                        }
                        _ => {
                            self.reset();
                        }
                    }
                }

                RouterState::Csi => {
                    if byte == 0x1b {
                        // ESC aborts current CSI, starts a new sequence
                        self.seq_bytes.clear();
                        self.seq_bytes.push(byte);
                        self.state = RouterState::Esc;
                    } else {
                        self.seq_bytes.push(byte);
                        match self.dec_parser.feed(byte) {
                            DecModeResult::Pending => {}
                            DecModeResult::Complete(action) => {
                                let raw_bytes = std::mem::take(&mut self.seq_bytes);
                                let event = match action {
                                    DecModeAction::Set { modes } => {
                                        DecModeEvent::Set { modes, raw_bytes }
                                    }
                                    DecModeAction::Reset { modes } => {
                                        DecModeEvent::Reset { modes, raw_bytes }
                                    }
                                };
                                events.push(SequenceEvent::DecMode(event));
                                self.state = RouterState::Ground;
                            }
                            DecModeResult::Reject { private } => {
                                if private {
                                    // Had `?` prefix — this is a DEC-private sequence
                                    // with an unrecognized final byte (e.g. DA1
                                    // response `CSI ? 62 c`). Don't misinterpret it.
                                    self.reset();
                                } else if byte == b'c' {
                                    // DA1 query with no params (CSI c)
                                    let raw_bytes = std::mem::take(&mut self.seq_bytes);
                                    events.push(SequenceEvent::PrimaryDA { raw_bytes });
                                    self.state = RouterState::Ground;
                                } else if byte.is_ascii_digit() {
                                    self.state = RouterState::CsiStandard;
                                    self.csi_param = (byte - b'0') as u16;
                                } else {
                                    self.reset();
                                }
                            }
                        }
                    }
                }

                RouterState::CsiStandard => {
                    if byte == 0x1b {
                        self.seq_bytes.clear();
                        self.seq_bytes.push(byte);
                        self.state = RouterState::Esc;
                    } else {
                        self.seq_bytes.push(byte);
                        if byte.is_ascii_digit() {
                            self.csi_param = self
                                .csi_param
                                .checked_mul(10)
                                .and_then(|v| v.checked_add((byte - b'0') as u16))
                                .unwrap_or(u16::MAX);
                        } else if byte == b'c' && self.csi_param == 0 {
                            // DA1 with explicit zero param (CSI 0 c)
                            let raw_bytes = std::mem::take(&mut self.seq_bytes);
                            events.push(SequenceEvent::PrimaryDA { raw_bytes });
                            self.state = RouterState::Ground;
                        } else if byte == b'J' && self.csi_param == 2 {
                            let raw_bytes = std::mem::take(&mut self.seq_bytes);
                            events.push(SequenceEvent::EraseDisplay { raw_bytes });
                            self.state = RouterState::Ground;
                        } else if byte == b'J' && self.csi_param == 3 {
                            let raw_bytes = std::mem::take(&mut self.seq_bytes);
                            events.push(SequenceEvent::ClearScrollback { raw_bytes });
                            self.state = RouterState::Ground;
                        } else {
                            self.reset();
                        }
                    }
                }

                RouterState::Osc => {
                    if byte == 0x1b {
                        // Could be ST (ESC \) or a new sequence
                        self.seq_bytes.push(byte);
                        self.state = RouterState::OscEsc;
                    } else {
                        self.seq_bytes.push(byte);
                        match self.osc_parser.feed(byte) {
                            OscResult::Pending => {}
                            OscResult::Complete => {
                                let raw_bytes = std::mem::take(&mut self.seq_bytes);
                                events.push(SequenceEvent::Osc(OscEvent { raw_bytes }));
                                self.state = RouterState::Ground;
                            }
                            OscResult::Reject => {
                                self.reset();
                            }
                        }
                    }
                }

                RouterState::OscEsc => {
                    if byte == b'\\' {
                        // ST (ESC \) terminates the OSC
                        self.seq_bytes.push(byte);
                        match self.osc_parser.finish() {
                            OscResult::Complete => {
                                let raw_bytes = std::mem::take(&mut self.seq_bytes);
                                events.push(SequenceEvent::Osc(OscEvent { raw_bytes }));
                                self.state = RouterState::Ground;
                            }
                            _ => {
                                self.reset();
                            }
                        }
                    } else if byte == 0x1b {
                        // Another ESC — abort OSC, start new sequence
                        self.seq_bytes.clear();
                        self.seq_bytes.push(byte);
                        self.state = RouterState::Esc;
                    } else {
                        // ESC wasn't ST — treat ESC + byte as new sequence start
                        self.seq_bytes.clear();
                        self.seq_bytes.push(0x1b);
                        self.seq_bytes.push(byte);
                        match byte {
                            b'[' => {
                                self.state = RouterState::Csi;
                                self.dec_parser.reset();
                            }
                            b']' => {
                                self.state = RouterState::Osc;
                                self.osc_parser.reset();
                            }
                            _ => {
                                self.reset();
                            }
                        }
                    }
                }
            }
        }
    }

    fn reset(&mut self) {
        self.state = RouterState::Ground;
        self.seq_bytes.clear();
        self.csi_param = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- DEC private mode tests --

    #[test]
    fn detects_alt_screen_enter() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?1049h");
        assert_eq!(events.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(ev.enters_alt_screen());
        assert!(ev.has_forwarded_mode());
        assert_eq!(ev.raw_bytes(), b"\x1b[?1049h");
    }

    #[test]
    fn detects_alt_screen_exit() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?1049l");
        assert_eq!(events.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(ev.exits_alt_screen());
        assert!(ev.has_forwarded_mode());
    }

    #[test]
    fn detects_mouse_tracking() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?1000h");
        assert_eq!(events.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(ev.has_forwarded_mode());
        assert!(!ev.enters_alt_screen());
        match ev {
            DecModeEvent::Set { modes, .. } => assert_eq!(modes, &[1000]),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn handles_compound_sequence() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?1049;1006h");
        assert_eq!(events.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(ev.enters_alt_screen());
        assert!(ev.has_forwarded_mode());
        match ev {
            DecModeEvent::Set { modes, .. } => assert_eq!(modes, &[1049, 1006]),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn handles_split_across_buffers() {
        let mut scanner = EscapeScanner::new();

        let events1 = scanner.scan(b"\x1b[?10");
        assert!(events1.is_empty());

        let events2 = scanner.scan(b"49h");
        assert_eq!(events2.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events2[0] else {
            panic!("expected DecMode");
        };
        assert!(ev.enters_alt_screen());
        assert_eq!(ev.raw_bytes(), b"\x1b[?1049h");
    }

    #[test]
    fn handles_split_at_every_byte() {
        let mut scanner = EscapeScanner::new();
        let seq = b"\x1b[?1049h";
        for (i, &byte) in seq.iter().enumerate() {
            let events = scanner.scan(&[byte]);
            if i < seq.len() - 1 {
                assert!(events.is_empty());
            } else {
                assert_eq!(events.len(), 1);
                let SequenceEvent::DecMode(ref ev) = events[0] else {
                    panic!("expected DecMode");
                };
                assert!(ev.enters_alt_screen());
            }
        }
    }

    #[test]
    fn ignores_non_dec_csi() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[1;31m");
        assert!(events.is_empty());
    }

    #[test]
    fn ignores_unknown_dec_modes() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?25l");
        assert_eq!(events.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(!ev.has_forwarded_mode());
    }

    #[test]
    fn multiple_sequences_in_one_buffer() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?1049h\x1b[?1006h");
        assert_eq!(events.len(), 2);
        let SequenceEvent::DecMode(ref ev0) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(ev0.enters_alt_screen());
        let SequenceEvent::DecMode(ref ev1) = events[1] else {
            panic!("expected DecMode");
        };
        match ev1 {
            DecModeEvent::Set { modes, .. } => assert_eq!(modes, &[1006]),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn sequences_interleaved_with_text() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"hello\x1b[?1049hworld\x1b[?1049l");
        assert_eq!(events.len(), 2);
        let SequenceEvent::DecMode(ref ev0) = events[0] else {
            panic!("expected DecMode");
        };
        let SequenceEvent::DecMode(ref ev1) = events[1] else {
            panic!("expected DecMode");
        };
        assert!(ev0.enters_alt_screen());
        assert!(ev1.exits_alt_screen());
    }

    #[test]
    fn aborts_on_invalid_byte_in_params() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?1049x");
        assert!(events.is_empty());
        // Scanner should be back in ground state and able to parse next sequence
        let events = scanner.scan(b"\x1b[?1049h");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn mode_47_is_alt_screen() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?47h");
        assert_eq!(events.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(ev.enters_alt_screen());
    }

    #[test]
    fn synchronized_output_mode() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?2026h");
        assert_eq!(events.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(ev.has_forwarded_mode());
        match ev {
            DecModeEvent::Set { modes, .. } => assert_eq!(modes, &[2026]),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn bracketed_paste_mode() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?2004h");
        assert_eq!(events.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(ev.has_forwarded_mode());
        match ev {
            DecModeEvent::Set { modes, .. } => assert_eq!(modes, &[2004]),
            _ => panic!("expected Set"),
        }
    }

    // -- OSC tests --

    #[test]
    fn osc_query_with_bel() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b]11;?\x07");
        assert_eq!(events.len(), 1);
        let SequenceEvent::Osc(ref ev) = events[0] else {
            panic!("expected Osc");
        };
        assert_eq!(ev.raw_bytes, b"\x1b]11;?\x07");
    }

    #[test]
    fn osc_query_with_st() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b]11;?\x1b\\");
        assert_eq!(events.len(), 1);
        let SequenceEvent::Osc(ref ev) = events[0] else {
            panic!("expected Osc");
        };
        assert_eq!(ev.raw_bytes, b"\x1b]11;?\x1b\\");
    }

    #[test]
    fn osc_non_query_ignored() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b]0;my title\x07");
        assert!(events.is_empty());
    }

    #[test]
    fn osc_split_across_buffers() {
        let mut scanner = EscapeScanner::new();

        let events1 = scanner.scan(b"\x1b]11");
        assert!(events1.is_empty());

        let events2 = scanner.scan(b";?\x07");
        assert_eq!(events2.len(), 1);
        let SequenceEvent::Osc(ref ev) = events2[0] else {
            panic!("expected Osc");
        };
        assert_eq!(ev.raw_bytes, b"\x1b]11;?\x07");
    }

    #[test]
    fn osc_split_at_every_byte() {
        let mut scanner = EscapeScanner::new();
        let seq = b"\x1b]11;?\x07";
        for (i, &byte) in seq.iter().enumerate() {
            let events = scanner.scan(&[byte]);
            if i < seq.len() - 1 {
                assert!(events.is_empty());
            } else {
                assert_eq!(events.len(), 1);
                assert!(matches!(events[0], SequenceEvent::Osc(_)));
            }
        }
    }

    #[test]
    fn mixed_dec_and_osc() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?1049h\x1b]11;?\x07\x1b[?1049l");
        assert_eq!(events.len(), 3);
        assert!(matches!(events[0], SequenceEvent::DecMode(_)));
        assert!(matches!(events[1], SequenceEvent::Osc(_)));
        assert!(matches!(events[2], SequenceEvent::DecMode(_)));
    }

    #[test]
    fn osc_c0_control_rejects() {
        let mut scanner = EscapeScanner::new();
        // NUL in the middle of OSC payload
        let events = scanner.scan(b"\x1b]11\x00?\x07");
        assert!(events.is_empty());
        // Scanner recovers
        let events = scanner.scan(b"\x1b]11;?\x07");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn osc_various_queries() {
        let mut scanner = EscapeScanner::new();
        // OSC 10 (foreground color query)
        let events = scanner.scan(b"\x1b]10;?\x07");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::Osc(_)));
    }

    // -- ESC mid-sequence recovery tests --

    #[test]
    fn esc_mid_csi_restarts_sequence() {
        let mut scanner = EscapeScanner::new();
        // ESC in the middle of a CSI aborts it and starts a new one
        let events = scanner.scan(b"\x1b[?123\x1b[?1049h");
        assert_eq!(events.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(ev.enters_alt_screen());
        assert_eq!(ev.raw_bytes(), b"\x1b[?1049h");
    }

    #[test]
    fn esc_mid_osc_starts_new_csi() {
        let mut scanner = EscapeScanner::new();
        // ESC [ in the middle of OSC aborts it and starts CSI
        let events = scanner.scan(b"\x1b]11\x1b[?1049h");
        assert_eq!(events.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(ev.enters_alt_screen());
    }

    #[test]
    fn esc_mid_osc_starts_new_osc() {
        let mut scanner = EscapeScanner::new();
        // ESC ] in the middle of OSC aborts it and starts new OSC
        let events = scanner.scan(b"\x1b]11\x1b]10;?\x07");
        assert_eq!(events.len(), 1);
        let SequenceEvent::Osc(ref ev) = events[0] else {
            panic!("expected Osc");
        };
        assert_eq!(ev.raw_bytes, b"\x1b]10;?\x07");
    }

    #[test]
    fn double_esc_restarts() {
        let mut scanner = EscapeScanner::new();
        // Double ESC: first is discarded, second starts sequence
        let events = scanner.scan(b"\x1b\x1b[?1049h");
        assert_eq!(events.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(ev.enters_alt_screen());
    }

    #[test]
    fn osc_st_split_across_buffers() {
        let mut scanner = EscapeScanner::new();

        let events1 = scanner.scan(b"\x1b]11;?\x1b");
        assert!(events1.is_empty());

        let events2 = scanner.scan(b"\\");
        assert_eq!(events2.len(), 1);
        let SequenceEvent::Osc(ref ev) = events2[0] else {
            panic!("expected Osc");
        };
        assert_eq!(ev.raw_bytes, b"\x1b]11;?\x1b\\");
    }

    // -- Clear scrollback (CSI 3 J) tests --

    #[test]
    fn detects_clear_scrollback() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[3J");
        assert_eq!(events.len(), 1);
        let SequenceEvent::ClearScrollback { ref raw_bytes } = events[0] else {
            panic!("expected ClearScrollback");
        };
        assert_eq!(raw_bytes, b"\x1b[3J");
    }

    #[test]
    fn clear_scrollback_split_across_buffers() {
        let mut scanner = EscapeScanner::new();

        let events1 = scanner.scan(b"\x1b[");
        assert!(events1.is_empty());

        let events2 = scanner.scan(b"3");
        assert!(events2.is_empty());

        let events3 = scanner.scan(b"J");
        assert_eq!(events3.len(), 1);
        assert!(matches!(events3[0], SequenceEvent::ClearScrollback { .. }));
    }

    #[test]
    fn ignores_other_ed_params() {
        let mut scanner = EscapeScanner::new();
        // CSI 1 J should not emit any event
        let events = scanner.scan(b"\x1b[1J");
        assert!(events.is_empty());
        // Scanner recovers
        let events = scanner.scan(b"\x1b[3J");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ClearScrollback { .. }));
    }

    #[test]
    fn detects_erase_display() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[2J");
        assert_eq!(events.len(), 1);
        let SequenceEvent::EraseDisplay { ref raw_bytes } = events[0] else {
            panic!("expected EraseDisplay");
        };
        assert_eq!(raw_bytes, b"\x1b[2J");
    }

    #[test]
    fn erase_display_split_across_buffers() {
        let mut scanner = EscapeScanner::new();

        let events1 = scanner.scan(b"\x1b[");
        assert!(events1.is_empty());

        let events2 = scanner.scan(b"2");
        assert!(events2.is_empty());

        let events3 = scanner.scan(b"J");
        assert_eq!(events3.len(), 1);
        assert!(matches!(events3[0], SequenceEvent::EraseDisplay { .. }));
    }

    #[test]
    fn erase_display_interleaved_with_clear_scrollback() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[2J\x1b[3J");
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], SequenceEvent::EraseDisplay { .. }));
        assert!(matches!(events[1], SequenceEvent::ClearScrollback { .. }));
    }

    #[test]
    fn clear_scrollback_interleaved_with_dec_mode() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?1049h\x1b[3J");
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], SequenceEvent::DecMode(_)));
        assert!(matches!(events[1], SequenceEvent::ClearScrollback { .. }));
    }

    // -- Primary Device Attributes (DA1) tests --

    #[test]
    fn detects_da1_no_params() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[c");
        assert_eq!(events.len(), 1);
        let SequenceEvent::PrimaryDA { ref raw_bytes } = events[0] else {
            panic!("expected PrimaryDA");
        };
        assert_eq!(raw_bytes, b"\x1b[c");
    }

    #[test]
    fn detects_da1_with_zero_param() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[0c");
        assert_eq!(events.len(), 1);
        let SequenceEvent::PrimaryDA { ref raw_bytes } = events[0] else {
            panic!("expected PrimaryDA");
        };
        assert_eq!(raw_bytes, b"\x1b[0c");
    }

    #[test]
    fn da1_split_across_buffers() {
        let mut scanner = EscapeScanner::new();

        let events1 = scanner.scan(b"\x1b[");
        assert!(events1.is_empty());

        let events2 = scanner.scan(b"c");
        assert_eq!(events2.len(), 1);
        assert!(matches!(events2[0], SequenceEvent::PrimaryDA { .. }));
    }

    #[test]
    fn da1_interleaved_with_text() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"hello\x1b[cworld");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::PrimaryDA { .. }));
    }

    #[test]
    fn da1_does_not_match_nonzero_param() {
        let mut scanner = EscapeScanner::new();
        // CSI 1 c is not DA1
        let events = scanner.scan(b"\x1b[1c");
        assert!(events.is_empty());
        // Scanner recovers
        let events = scanner.scan(b"\x1b[c");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::PrimaryDA { .. }));
    }

    #[test]
    fn da1_response_not_misdetected_as_query() {
        let mut scanner = EscapeScanner::new();
        // CSI ? 62 c is a DA1 *response*, not a query — must not emit PrimaryDA
        let events = scanner.scan(b"\x1b[?62c");
        assert!(events.is_empty());
        // Scanner recovers
        let events = scanner.scan(b"\x1b[c");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::PrimaryDA { .. }));
    }

    // -- Keypad mode (DECKPAM/DECKPNM) tests --

    #[test]
    fn detects_deckpam() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b=");
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            SequenceEvent::KeypadMode { application: true }
        ));
    }

    #[test]
    fn detects_deckpnm() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b>");
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            SequenceEvent::KeypadMode { application: false }
        ));
    }

    #[test]
    fn keypad_mode_split_across_buffers() {
        let mut scanner = EscapeScanner::new();

        let events1 = scanner.scan(b"\x1b");
        assert!(events1.is_empty());

        let events2 = scanner.scan(b"=");
        assert_eq!(events2.len(), 1);
        assert!(matches!(
            events2[0],
            SequenceEvent::KeypadMode { application: true }
        ));
    }

    #[test]
    fn smkx_sequence() {
        let mut scanner = EscapeScanner::new();
        // smkx = \x1b[?1h\x1b=
        let events = scanner.scan(b"\x1b[?1h\x1b=");
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], SequenceEvent::DecMode(_)));
        assert!(matches!(
            events[1],
            SequenceEvent::KeypadMode { application: true }
        ));
    }

    #[test]
    fn rmkx_sequence() {
        let mut scanner = EscapeScanner::new();
        // rmkx = \x1b[?1l\x1b>
        let events = scanner.scan(b"\x1b[?1l\x1b>");
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], SequenceEvent::DecMode(_)));
        assert!(matches!(
            events[1],
            SequenceEvent::KeypadMode { application: false }
        ));
    }
}
