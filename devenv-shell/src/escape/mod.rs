//! Stateful byte-level scanner for escape sequences in raw PTY output.
//!
//! Detects DEC private mode sequences, CSI queries, DCS sequences, and
//! OSC sequences so they can be forwarded to the real terminal (avt consumes
//! them internally).

mod dec_mode;
mod osc;

use dec_mode::{DecModeAction, DecModeParser, DecModeResult};
use osc::{OscParser, OscResult};

/// DEC private modes that should be forwarded to the real terminal.
///
/// NOTE: Mode 2048 (in-band size reports) is intentionally excluded.
/// The status line reserves 1 row, so the PTY has `rows - 1` rows while
/// the real terminal has `rows`. Forwarding mode 2048 would cause the
/// terminal to send resize notifications with the real (larger) size,
/// making programs like nvim think they have an extra row and draw over
/// the status line.
const FORWARDED_MODES: &[u16] = &[
    1, // cursor key mode (DECCKM) — applications use smkx/rmkx to toggle
    9, // X10 mouse mode (legacy)
    47, 1047, 1049, // alternate screen
    1000, 1002, 1003, // mouse tracking
    1005, 1006, 1015, 1016, // mouse encoding (including SGR pixels)
    2004, // bracketed paste
    1004, // focus events
    2026, // synchronized output
    2031, // color scheme reporting
];

/// Modes that control alternate screen buffer.
const ALT_SCREEN_MODES: &[u16] = &[47, 1047, 1049];

/// Mode 2048: in-band resize reports. Not forwarded to the real terminal
/// because the PTY has fewer rows than the real terminal (status line).
const IN_BAND_RESIZE_MODE: u16 = 2048;

/// Modes that need explicit reset on session exit. Excludes alternate screen
/// modes (handled via `LeaveAlternateScreen`) and synchronized output (mode
/// 2026, managed per-frame by the renderer).
pub const CLEANUP_MODES: &[u16] = &[
    1, // cursor key mode (DECCKM)
    9, // X10 mouse mode
    1000, 1002, 1003, // mouse tracking
    1005, 1006, 1015, 1016, // mouse encoding
    2004, // bracketed paste
    1004, // focus events
    2031, // color scheme reporting
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

    /// Whether this event enables in-band resize (mode 2048).
    pub fn enables_in_band_resize(&self) -> bool {
        matches!(self, DecModeEvent::Set { modes, .. } if modes.contains(&IN_BAND_RESIZE_MODE))
    }

    /// Whether this event disables in-band resize (mode 2048).
    pub fn disables_in_band_resize(&self) -> bool {
        matches!(self, DecModeEvent::Reset { modes, .. } if modes.contains(&IN_BAND_RESIZE_MODE))
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
    /// CSI sequence that should be forwarded verbatim to the real terminal.
    /// Covers terminal queries (DA1, DA2, DA3, DSR, CPR, DECRQM, XTVERSION,
    /// XTWINOPS, etc.) and pass-through commands (DECSCUSR cursor shape).
    ForwardCsi {
        raw_bytes: Vec<u8>,
    },
    /// DCS sequence that should be forwarded verbatim (XTGETTCAP, DECRQSS).
    ForwardDcs {
        raw_bytes: Vec<u8>,
    },
    /// `ESC =` (DECKPAM) or `ESC >` (DECKPNM) — keypad application/numeric mode.
    /// Part of `smkx`/`rmkx` terminfo capabilities alongside DECCKM.
    KeypadMode {
        /// `true` = application mode (DECKPAM, `ESC =`),
        /// `false` = numeric mode (DECKPNM, `ESC >`).
        application: bool,
    },
    /// Kitty keyboard protocol push/pop/set.
    KittyKeyboard {
        raw_bytes: Vec<u8>,
        /// +1 for push, -1 for pop, 0 for set.
        stack_delta: i8,
    },
    /// XTMODIFYOTHERKEYS set/reset.
    ModifyOtherKeys {
        raw_bytes: Vec<u8>,
        enabled: bool,
    },
    /// CSI 18 t — program is querying text area size in characters.
    /// The session responds with PTY dimensions (not real terminal size).
    TextAreaSizeQuery,
}

/// Maximum number of CSI parameters to accumulate.
const MAX_CSI_PARAMS: usize = 16;
/// Maximum number of CSI intermediate bytes.
const MAX_CSI_INTERMEDIATES: usize = 2;
/// Maximum DCS payload size before giving up.
const MAX_DCS_PAYLOAD: usize = 4096;

/// Classification result for a complete CSI sequence.
enum CsiClass {
    /// Forward verbatim to the real terminal (query).
    Forward,
    /// CSI 2 J — erase display (intercepted for renderer).
    EraseDisplay,
    /// CSI 3 J — clear scrollback (intercepted for renderer).
    ClearScrollback,
    /// Kitty keyboard push/pop/set.
    KittyKeyboard { stack_delta: i8 },
    /// XTMODIFYOTHERKEYS set/reset.
    ModifyOtherKeys { enabled: bool },
    /// CSI 18 t — text area size query. Intercepted so we can respond
    /// with PTY dimensions instead of the real terminal size.
    TextAreaSizeQuery,
    /// AVT handles it, no forwarding needed.
    Ignore,
}

/// Classify a complete CSI sequence by its intermediates, params, and final byte.
fn classify_csi(
    intermediates: &[u8],
    params: &[u16],
    param_count: usize,
    final_byte: u8,
) -> CsiClass {
    let p = &params[..param_count];
    let first = p.first().copied().unwrap_or(0);

    match (intermediates, final_byte) {
        // Queries — forward to real terminal
        ([], b'c') if first == 0 => CsiClass::Forward, // DA1
        ([b'>'], b'c') => CsiClass::Forward,           // DA2
        ([b'='], b'c') => CsiClass::Forward,           // DA3
        ([], b'n') if first == 5 || first == 6 => CsiClass::Forward, // DSR/CPR
        ([b'?'], b'n') => CsiClass::Forward,           // DEC DSR (? 6 n, ? 996 n)
        ([b'>'], b'q') => CsiClass::Forward,           // XTVERSION
        ([b'?'], b'u') => CsiClass::Forward,           // Kitty KB query
        ([b'?', b'$'], b'p') => CsiClass::Forward,     // DECRQM DEC mode
        ([b'$'], b'p') => CsiClass::Forward,           // DECRQM ANSI mode

        // XTWINOPS queries (see FORWARDED_MODES doc comment for why
        // size-reporting queries are not forwarded to the real terminal).
        ([], b't') if first == 18 => CsiClass::TextAreaSizeQuery,
        ([], b't') if matches!(first, 16 | 21) => CsiClass::Forward,

        // Erase — intercept for renderer coordination
        ([], b'J') if first == 2 => CsiClass::EraseDisplay,
        ([], b'J') if first == 3 => CsiClass::ClearScrollback,

        // Kitty keyboard protocol
        ([b'>'], b'u') => CsiClass::KittyKeyboard { stack_delta: 1 }, // push
        ([b'<'], b'u') => CsiClass::KittyKeyboard { stack_delta: -1 }, // pop
        ([b'='], b'u') => CsiClass::KittyKeyboard { stack_delta: 0 }, // set

        // XTMODIFYOTHERKEYS
        // CSI > m (no params) or CSI > 0 m → reset (legacy)
        // CSI > N m where N > 0 → set (may enable depending on sub-params)
        ([b'>'], b'm') => CsiClass::ModifyOtherKeys { enabled: first > 0 },
        // CSI > n → reset to default
        ([b'>'], b'n') => CsiClass::ModifyOtherKeys { enabled: false },

        // DECSCUSR — cursor shape (block, bar, underline).
        // Must be forwarded to the real terminal so programs like
        // neovim can change cursor shape between modes.
        ([b' '], b'q') => CsiClass::Forward,

        // XTSHIFTESCAPE — configure whether shift modifier is reported
        // in mouse events. Must be forwarded alongside mouse tracking
        // DEC modes (1000, 1002, etc.).
        ([b'>'], b's') => CsiClass::Forward,

        _ => CsiClass::Ignore,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RouterState {
    Ground,
    Esc,
    Csi,
    /// Full CSI parameter accumulator.
    CsiParams,
    Osc,
    /// ESC seen while in OSC — could be ST (`ESC \`) or a new sequence.
    OscEsc,
    /// DCS sequence accumulator (`ESC P ... ST`).
    Dcs,
    /// ESC seen while in DCS — could be ST or start of new sequence.
    DcsEsc,
}

/// Full CSI parameter accumulator.
struct CsiParamState {
    intermediates: [u8; MAX_CSI_INTERMEDIATES],
    intermediate_count: usize,
    params: [u16; MAX_CSI_PARAMS],
    param_count: usize,
    current_param: Option<u16>,
    /// Whether this CSI has a `?` prefix (DEC private mode).
    has_question: bool,
    /// Whether the DEC mode parser has rejected (stop feeding it).
    dec_rejected: bool,
}

impl CsiParamState {
    fn reset(&mut self) {
        self.intermediate_count = 0;
        self.param_count = 0;
        self.current_param = None;
        self.has_question = false;
        self.dec_rejected = false;
    }

    /// Feed a byte. Returns true if the byte was consumed (not a final byte).
    /// Returns false if this is a final byte (0x40-0x7E).
    fn feed(&mut self, byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => {
                let cur = self.current_param.get_or_insert(0);
                *cur = cur
                    .checked_mul(10)
                    .and_then(|v| v.checked_add((byte - b'0') as u16))
                    .unwrap_or(u16::MAX);
                None
            }
            b';' => {
                if self.param_count < MAX_CSI_PARAMS {
                    self.params[self.param_count] = self.current_param.unwrap_or(0);
                    self.param_count += 1;
                    self.current_param = Some(0);
                }
                None
            }
            // Intermediate bytes (0x20-0x2F) and private-use markers
            b'?' => {
                self.has_question = true;
                if self.intermediate_count < MAX_CSI_INTERMEDIATES {
                    self.intermediates[self.intermediate_count] = byte;
                    self.intermediate_count += 1;
                }
                None
            }
            b'>' | b'=' | b'<' => {
                if self.intermediate_count < MAX_CSI_INTERMEDIATES {
                    self.intermediates[self.intermediate_count] = byte;
                    self.intermediate_count += 1;
                }
                None
            }
            // Intermediate bytes (0x20-0x2F, includes space and $).
            // Stored for use in classification.
            0x20..=0x2F => {
                if self.intermediate_count < MAX_CSI_INTERMEDIATES {
                    self.intermediates[self.intermediate_count] = byte;
                    self.intermediate_count += 1;
                }
                None
            }
            // Final byte (0x40-0x7E)
            0x40..=0x7E => {
                // Finalize the last parameter
                if self.param_count < MAX_CSI_PARAMS
                    && let Some(val) = self.current_param
                {
                    self.params[self.param_count] = val;
                    self.param_count += 1;
                }
                Some(byte)
            }
            _ => None,
        }
    }

    /// Get the intermediates slice for classification.
    fn intermediates(&self) -> &[u8] {
        &self.intermediates[..self.intermediate_count]
    }
}

/// Byte-level scanner that detects escape sequences in raw PTY output.
///
/// Routes `ESC [` to the CSI parameter accumulator, `ESC ]` to the
/// OSC parser, and `ESC P` to the DCS accumulator.
/// State persists across calls to handle sequences split across buffer boundaries.
pub struct EscapeScanner {
    state: RouterState,
    seq_bytes: Vec<u8>,
    dec_parser: DecModeParser,
    osc_parser: OscParser,
    csi_params: CsiParamState,
}

impl EscapeScanner {
    pub fn new() -> Self {
        Self {
            state: RouterState::Ground,
            seq_bytes: Vec::new(),
            dec_parser: DecModeParser::new(),
            osc_parser: OscParser::new(),
            csi_params: CsiParamState {
                intermediates: [0; MAX_CSI_INTERMEDIATES],
                intermediate_count: 0,
                params: [0; MAX_CSI_PARAMS],
                param_count: 0,
                current_param: None,
                has_question: false,
                dec_rejected: false,
            },
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
                            self.csi_params.reset();
                        }
                        b']' => {
                            self.state = RouterState::Osc;
                            self.osc_parser.reset();
                        }
                        b'P' => {
                            self.state = RouterState::Dcs;
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
                        self.seq_bytes.clear();
                        self.seq_bytes.push(byte);
                        self.state = RouterState::Esc;
                    } else {
                        self.seq_bytes.push(byte);

                        // First byte after CSI determines the path:
                        // `?` → DEC private mode (use existing DecModeParser for set/reset)
                        // Other → general CSI parameter accumulator
                        if byte == b'?' {
                            // Feed into both: DecModeParser for h/l, CsiParams for queries
                            self.dec_parser.feed(byte);
                            self.csi_params.feed(byte);
                            self.state = RouterState::CsiParams;
                        } else if let Some(final_byte) = self.csi_params.feed(byte) {
                            // Immediate final byte (e.g. CSI c)
                            self.handle_csi_final(final_byte, events);
                        } else if byte.is_ascii_digit()
                            || byte == b';'
                            || matches!(byte, b'>' | b'=' | b'<' | b'$')
                            || matches!(byte, 0x20..=0x2F)
                        {
                            self.state = RouterState::CsiParams;
                        } else {
                            self.reset();
                        }
                    }
                }

                RouterState::CsiParams => {
                    if byte == 0x1b {
                        self.seq_bytes.clear();
                        self.seq_bytes.push(byte);
                        self.state = RouterState::Esc;
                    } else {
                        self.seq_bytes.push(byte);

                        // Feed into DecModeParser if we have `?` and it hasn't rejected yet
                        if self.csi_params.has_question && !self.csi_params.dec_rejected {
                            let dec_result = self.dec_parser.feed(byte);
                            match dec_result {
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
                                    continue;
                                }
                                DecModeResult::Reject { private: true } => {
                                    // Had `?` prefix but wasn't h/l.
                                    // Stop feeding DecModeParser, fall through
                                    // to CsiParams for queries like CSI ? u, CSI ? $ p
                                    self.csi_params.dec_rejected = true;
                                }
                                DecModeResult::Reject { private: false } => {
                                    self.reset();
                                    continue;
                                }
                            }
                        }

                        if let Some(final_byte) = self.csi_params.feed(byte) {
                            self.handle_csi_final(final_byte, events);
                        }
                    }
                }

                RouterState::Osc => {
                    if byte == 0x1b {
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
                        self.seq_bytes.clear();
                        self.seq_bytes.push(byte);
                        self.state = RouterState::Esc;
                    } else {
                        self.seq_bytes.clear();
                        self.seq_bytes.push(0x1b);
                        self.seq_bytes.push(byte);
                        match byte {
                            b'[' => {
                                self.state = RouterState::Csi;
                                self.dec_parser.reset();
                                self.csi_params.reset();
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

                RouterState::Dcs => {
                    self.seq_bytes.push(byte);
                    if byte == 0x1b {
                        self.state = RouterState::DcsEsc;
                    } else if self.seq_bytes.len() > MAX_DCS_PAYLOAD {
                        self.reset();
                    }
                }

                RouterState::DcsEsc => {
                    if byte == b'\\' {
                        // ST (ESC \) terminates the DCS
                        self.seq_bytes.push(byte);
                        let raw_bytes = std::mem::take(&mut self.seq_bytes);
                        events.push(SequenceEvent::ForwardDcs { raw_bytes });
                        self.state = RouterState::Ground;
                    } else if byte == 0x1b {
                        // Another ESC — abort DCS, start new sequence
                        self.seq_bytes.clear();
                        self.seq_bytes.push(byte);
                        self.state = RouterState::Esc;
                    } else {
                        // ESC wasn't ST — abort DCS. The ESC + this byte
                        // could be a new escape sequence.
                        self.seq_bytes.clear();
                        self.seq_bytes.push(0x1b);
                        self.seq_bytes.push(byte);
                        match byte {
                            b'[' => {
                                self.state = RouterState::Csi;
                                self.dec_parser.reset();
                                self.csi_params.reset();
                            }
                            b']' => {
                                self.state = RouterState::Osc;
                                self.osc_parser.reset();
                            }
                            b'P' => {
                                self.state = RouterState::Dcs;
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

    /// Handle a complete CSI sequence (final byte received).
    fn handle_csi_final(&mut self, final_byte: u8, events: &mut Vec<SequenceEvent>) {
        let class = classify_csi(
            self.csi_params.intermediates(),
            &self.csi_params.params,
            self.csi_params.param_count,
            final_byte,
        );

        match class {
            CsiClass::Forward => {
                let raw_bytes = std::mem::take(&mut self.seq_bytes);
                events.push(SequenceEvent::ForwardCsi { raw_bytes });
            }
            CsiClass::EraseDisplay => {
                let raw_bytes = std::mem::take(&mut self.seq_bytes);
                events.push(SequenceEvent::EraseDisplay { raw_bytes });
            }
            CsiClass::ClearScrollback => {
                let raw_bytes = std::mem::take(&mut self.seq_bytes);
                events.push(SequenceEvent::ClearScrollback { raw_bytes });
            }
            CsiClass::KittyKeyboard { stack_delta } => {
                let raw_bytes = std::mem::take(&mut self.seq_bytes);
                events.push(SequenceEvent::KittyKeyboard {
                    raw_bytes,
                    stack_delta,
                });
            }
            CsiClass::ModifyOtherKeys { enabled } => {
                let raw_bytes = std::mem::take(&mut self.seq_bytes);
                events.push(SequenceEvent::ModifyOtherKeys { raw_bytes, enabled });
            }
            CsiClass::TextAreaSizeQuery => {
                events.push(SequenceEvent::TextAreaSizeQuery);
            }
            CsiClass::Ignore => {}
        }
        self.state = RouterState::Ground;
    }

    fn reset(&mut self) {
        self.state = RouterState::Ground;
        self.seq_bytes.clear();
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
    fn osc_non_query_forwarded() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b]0;my title\x07");
        assert_eq!(events.len(), 1);
        let SequenceEvent::Osc(ref ev) = events[0] else {
            panic!("expected Osc");
        };
        assert_eq!(ev.raw_bytes, b"\x1b]0;my title\x07");
    }

    #[test]
    fn osc_hyperlink_forwarded() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b]8;;https://example.com\x07link\x1b]8;;\x07");
        assert_eq!(events.len(), 2);

        let SequenceEvent::Osc(ref open) = events[0] else {
            panic!("expected opening Osc");
        };
        assert_eq!(open.raw_bytes, b"\x1b]8;;https://example.com\x07");

        let SequenceEvent::Osc(ref close) = events[1] else {
            panic!("expected closing Osc");
        };
        assert_eq!(close.raw_bytes, b"\x1b]8;;\x07");
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
        let SequenceEvent::ForwardCsi { ref raw_bytes } = events[0] else {
            panic!("expected ForwardCsi, got {:?}", events[0]);
        };
        assert_eq!(raw_bytes, b"\x1b[c");
    }

    #[test]
    fn detects_da1_with_zero_param() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[0c");
        assert_eq!(events.len(), 1);
        let SequenceEvent::ForwardCsi { ref raw_bytes } = events[0] else {
            panic!("expected ForwardCsi, got {:?}", events[0]);
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
        assert!(matches!(events2[0], SequenceEvent::ForwardCsi { .. }));
    }

    #[test]
    fn da1_interleaved_with_text() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"hello\x1b[cworld");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ForwardCsi { .. }));
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
        assert!(matches!(events[0], SequenceEvent::ForwardCsi { .. }));
    }

    #[test]
    fn da1_response_not_misdetected_as_query() {
        let mut scanner = EscapeScanner::new();
        // CSI ? 62 c is a DA1 *response*, not a query — must not emit ForwardCsi
        let events = scanner.scan(b"\x1b[?62c");
        assert!(events.is_empty());
        // Scanner recovers
        let events = scanner.scan(b"\x1b[c");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ForwardCsi { .. }));
    }

    // -- Device Status Report (DSR/CPR) tests --

    #[test]
    fn detects_cpr_request() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[6n");
        assert_eq!(events.len(), 1);
        let SequenceEvent::ForwardCsi { ref raw_bytes } = events[0] else {
            panic!("expected ForwardCsi, got {:?}", events[0]);
        };
        assert_eq!(raw_bytes, b"\x1b[6n");
    }

    #[test]
    fn detects_dsr_request() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[5n");
        assert_eq!(events.len(), 1);
        let SequenceEvent::ForwardCsi { ref raw_bytes } = events[0] else {
            panic!("expected ForwardCsi, got {:?}", events[0]);
        };
        assert_eq!(raw_bytes, b"\x1b[5n");
    }

    #[test]
    fn cpr_split_across_buffers() {
        let mut scanner = EscapeScanner::new();

        let events1 = scanner.scan(b"\x1b[");
        assert!(events1.is_empty());

        let events2 = scanner.scan(b"6");
        assert!(events2.is_empty());

        let events3 = scanner.scan(b"n");
        assert_eq!(events3.len(), 1);
        assert!(matches!(events3[0], SequenceEvent::ForwardCsi { .. }));
    }

    #[test]
    fn cpr_interleaved_with_text() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"hello\x1b[6nworld");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ForwardCsi { .. }));
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

    // -- New CSI query tests --

    #[test]
    fn detects_da2() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[>c");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ForwardCsi { .. }));
    }

    #[test]
    fn detects_da3() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[=c");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ForwardCsi { .. }));
    }

    #[test]
    fn detects_xtversion() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[>q");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ForwardCsi { .. }));
    }

    #[test]
    fn detects_kitty_keyboard_query() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?u");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ForwardCsi { .. }));
    }

    #[test]
    fn detects_dec_dsr() {
        let mut scanner = EscapeScanner::new();
        // CSI ? 6 n — origin-mode-aware CPR
        let events = scanner.scan(b"\x1b[?6n");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ForwardCsi { .. }));
    }

    #[test]
    fn ignores_xtwinops_pixel_size() {
        // CSI 14 t reports real terminal size; not forwarded (see FORWARDED_MODES).
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[14t");
        assert!(events.is_empty());
    }

    #[test]
    fn detects_xtwinops_cell_size() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[16t");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ForwardCsi { .. }));
    }

    #[test]
    fn intercepts_xtwinops_char_size() {
        // CSI 18 t is intercepted so the session can respond with PTY dimensions.
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[18t");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::TextAreaSizeQuery));
    }

    #[test]
    fn detects_decrqm_dec() {
        let mut scanner = EscapeScanner::new();
        // CSI ? 1 $ p — query DEC mode 1
        let events = scanner.scan(b"\x1b[?1$p");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ForwardCsi { .. }));
    }

    #[test]
    fn detects_decrqm_ansi() {
        let mut scanner = EscapeScanner::new();
        // CSI 4 $ p — query ANSI mode 4
        let events = scanner.scan(b"\x1b[4$p");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ForwardCsi { .. }));
    }

    // -- Kitty keyboard protocol tests --

    #[test]
    fn detects_kitty_keyboard_push() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[>1u");
        assert_eq!(events.len(), 1);
        let SequenceEvent::KittyKeyboard {
            stack_delta,
            ref raw_bytes,
        } = events[0]
        else {
            panic!("expected KittyKeyboard, got {:?}", events[0]);
        };
        assert_eq!(stack_delta, 1);
        assert_eq!(raw_bytes, b"\x1b[>1u");
    }

    #[test]
    fn detects_kitty_keyboard_pop() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[<u");
        assert_eq!(events.len(), 1);
        let SequenceEvent::KittyKeyboard { stack_delta, .. } = events[0] else {
            panic!("expected KittyKeyboard, got {:?}", events[0]);
        };
        assert_eq!(stack_delta, -1);
    }

    #[test]
    fn detects_kitty_keyboard_set() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[=1;2u");
        assert_eq!(events.len(), 1);
        let SequenceEvent::KittyKeyboard { stack_delta, .. } = events[0] else {
            panic!("expected KittyKeyboard, got {:?}", events[0]);
        };
        assert_eq!(stack_delta, 0);
    }

    // -- XTMODIFYOTHERKEYS tests --

    #[test]
    fn detects_xtmodifyotherkeys_enable() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[>4m");
        assert_eq!(events.len(), 1);
        let SequenceEvent::ModifyOtherKeys {
            enabled,
            ref raw_bytes,
        } = events[0]
        else {
            panic!("expected ModifyOtherKeys, got {:?}", events[0]);
        };
        assert!(enabled);
        assert_eq!(raw_bytes, b"\x1b[>4m");
    }

    #[test]
    fn detects_xtmodifyotherkeys_disable() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[>n");
        assert_eq!(events.len(), 1);
        let SequenceEvent::ModifyOtherKeys { enabled, .. } = events[0] else {
            panic!("expected ModifyOtherKeys, got {:?}", events[0]);
        };
        assert!(!enabled);
    }

    #[test]
    fn xtmodifyotherkeys_no_params_is_reset() {
        let mut scanner = EscapeScanner::new();
        // CSI > m with no params resets to legacy (disabled)
        let events = scanner.scan(b"\x1b[>m");
        assert_eq!(events.len(), 1);
        let SequenceEvent::ModifyOtherKeys { enabled, .. } = events[0] else {
            panic!("expected ModifyOtherKeys, got {:?}", events[0]);
        };
        assert!(!enabled);
    }

    #[test]
    fn xtmodifyotherkeys_zero_param_is_reset() {
        let mut scanner = EscapeScanner::new();
        // CSI > 0 m resets to legacy (disabled)
        let events = scanner.scan(b"\x1b[>0m");
        assert_eq!(events.len(), 1);
        let SequenceEvent::ModifyOtherKeys { enabled, .. } = events[0] else {
            panic!("expected ModifyOtherKeys, got {:?}", events[0]);
        };
        assert!(!enabled);
    }

    // -- DCS tests --

    #[test]
    fn detects_dcs_xtgettcap() {
        let mut scanner = EscapeScanner::new();
        // DCS + q 544e ST
        let events = scanner.scan(b"\x1bP+q544e\x1b\\");
        assert_eq!(events.len(), 1);
        let SequenceEvent::ForwardDcs { ref raw_bytes } = events[0] else {
            panic!("expected ForwardDcs, got {:?}", events[0]);
        };
        assert_eq!(raw_bytes, b"\x1bP+q544e\x1b\\");
    }

    #[test]
    fn detects_dcs_decrqss() {
        let mut scanner = EscapeScanner::new();
        // DCS $ q m ST (query SGR)
        let events = scanner.scan(b"\x1bP$qm\x1b\\");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ForwardDcs { .. }));
    }

    #[test]
    fn dcs_split_across_buffers() {
        let mut scanner = EscapeScanner::new();

        let events1 = scanner.scan(b"\x1bP+q54");
        assert!(events1.is_empty());

        let events2 = scanner.scan(b"4e\x1b\\");
        assert_eq!(events2.len(), 1);
        assert!(matches!(events2[0], SequenceEvent::ForwardDcs { .. }));
    }

    #[test]
    fn dcs_esc_mid_sequence_restarts() {
        let mut scanner = EscapeScanner::new();
        // ESC P starts DCS, then ESC [ starts a CSI — DCS is aborted
        let events = scanner.scan(b"\x1bPdata\x1b[c");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ForwardCsi { .. }));
    }

    // -- New DEC mode tests --

    #[test]
    fn mouse_sgr_pixels_mode() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?1016h");
        assert_eq!(events.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(ev.has_forwarded_mode());
    }

    #[test]
    fn color_scheme_reporting_mode() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?2031h");
        assert_eq!(events.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(ev.has_forwarded_mode());
    }

    #[test]
    fn in_band_size_reports_not_forwarded() {
        // Mode 2048 is excluded from FORWARDED_MODES (see its doc comment).
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?2048h");
        assert_eq!(events.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(!ev.has_forwarded_mode());
    }

    #[test]
    fn x10_mouse_mode() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?9h");
        assert_eq!(events.len(), 1);
        let SequenceEvent::DecMode(ref ev) = events[0] else {
            panic!("expected DecMode");
        };
        assert!(ev.has_forwarded_mode());
    }

    // -- DECSCUSR (cursor shape) tests --

    #[test]
    fn detects_decscusr_block_cursor() {
        let mut scanner = EscapeScanner::new();
        // CSI 2 SP q — steady block cursor
        let events = scanner.scan(b"\x1b[2 q");
        assert_eq!(events.len(), 1);
        let SequenceEvent::ForwardCsi { ref raw_bytes } = events[0] else {
            panic!("expected ForwardCsi, got {:?}", events[0]);
        };
        assert_eq!(raw_bytes, b"\x1b[2 q");
    }

    #[test]
    fn detects_decscusr_bar_cursor() {
        let mut scanner = EscapeScanner::new();
        // CSI 6 SP q — steady bar cursor
        let events = scanner.scan(b"\x1b[6 q");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ForwardCsi { .. }));
    }

    #[test]
    fn detects_decscusr_default_cursor() {
        let mut scanner = EscapeScanner::new();
        // CSI 0 SP q — default cursor shape
        let events = scanner.scan(b"\x1b[0 q");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SequenceEvent::ForwardCsi { .. }));
    }

    #[test]
    fn decscusr_split_across_buffers() {
        let mut scanner = EscapeScanner::new();

        let events1 = scanner.scan(b"\x1b[2");
        assert!(events1.is_empty());

        let events2 = scanner.scan(b" ");
        assert!(events2.is_empty());

        let events3 = scanner.scan(b"q");
        assert_eq!(events3.len(), 1);
        assert!(matches!(events3[0], SequenceEvent::ForwardCsi { .. }));
    }

    #[test]
    fn decscusr_interleaved_with_alt_screen() {
        let mut scanner = EscapeScanner::new();
        let events = scanner.scan(b"\x1b[?1049h\x1b[2 q");
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], SequenceEvent::DecMode(_)));
        assert!(matches!(events[1], SequenceEvent::ForwardCsi { .. }));
    }

    // -- XTSHIFTESCAPE tests --

    #[test]
    fn forwards_xtshiftescape() {
        let mut scanner = EscapeScanner::new();

        // CSI > 1 s — enable shift reporting
        let events = scanner.scan(b"\x1b[>1s");
        assert_eq!(events.len(), 1);
        let SequenceEvent::ForwardCsi { ref raw_bytes } = events[0] else {
            panic!("expected ForwardCsi, got {:?}", events[0]);
        };
        assert_eq!(raw_bytes, b"\x1b[>1s");

        // CSI > 0 s — disable shift reporting
        let events = scanner.scan(b"\x1b[>0s");
        assert_eq!(events.len(), 1);
        let SequenceEvent::ForwardCsi { ref raw_bytes } = events[0] else {
            panic!("expected ForwardCsi, got {:?}", events[0]);
        };
        assert_eq!(raw_bytes, b"\x1b[>0s");
    }
}
