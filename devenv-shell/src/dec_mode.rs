//! Stateful byte-level scanner for DEC private mode sequences.
//!
//! Detects `CSI ? <params> h/l` in raw PTY output so they can be
//! forwarded to the real terminal (avt consumes them internally).

/// DEC private modes that should be forwarded to the real terminal.
const FORWARDED_MODES: &[u16] = &[
    47, 1047, 1049, // alternate screen
    1000, 1002, 1003, // mouse tracking
    1005, 1006, 1015, // mouse encoding
    2004, // bracketed paste
    1004, // focus events
];

/// Modes that control alternate screen buffer.
const ALT_SCREEN_MODES: &[u16] = &[47, 1047, 1049];

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanState {
    Ground,
    Esc,
    Csi,
    DecParam,
}

/// Byte-level scanner that detects `CSI ? <params> h/l` sequences
/// in raw PTY output. State persists across calls to handle sequences
/// split across buffer boundaries.
pub struct DecModeScanner {
    state: ScanState,
    params: Vec<u16>,
    current_param: u16,
    seq_bytes: Vec<u8>,
}

impl DecModeScanner {
    pub fn new() -> Self {
        Self {
            state: ScanState::Ground,
            params: Vec::new(),
            current_param: 0,
            seq_bytes: Vec::new(),
        }
    }

    /// Scan a chunk of raw PTY output and return any DEC private mode events found.
    pub fn scan(&mut self, data: &[u8]) -> Vec<DecModeEvent> {
        let mut events = Vec::new();

        for &byte in data {
            match self.state {
                ScanState::Ground => {
                    if byte == 0x1b {
                        self.state = ScanState::Esc;
                        self.seq_bytes.clear();
                        self.seq_bytes.push(byte);
                    }
                }

                ScanState::Esc => {
                    self.seq_bytes.push(byte);
                    if byte == b'[' {
                        self.state = ScanState::Csi;
                    } else {
                        self.reset();
                    }
                }

                ScanState::Csi => {
                    self.seq_bytes.push(byte);
                    if byte == b'?' {
                        self.state = ScanState::DecParam;
                        self.params.clear();
                        self.current_param = 0;
                    } else {
                        self.reset();
                    }
                }

                ScanState::DecParam => {
                    self.seq_bytes.push(byte);
                    match byte {
                        b'0'..=b'9' => {
                            self.current_param = self
                                .current_param
                                .saturating_mul(10)
                                .saturating_add((byte - b'0') as u16);
                        }
                        b';' => {
                            self.params.push(self.current_param);
                            self.current_param = 0;
                        }
                        b'h' => {
                            self.params.push(self.current_param);
                            let modes = std::mem::take(&mut self.params);
                            let raw_bytes = std::mem::take(&mut self.seq_bytes);
                            events.push(DecModeEvent::Set { modes, raw_bytes });
                            self.state = ScanState::Ground;
                            self.current_param = 0;
                        }
                        b'l' => {
                            self.params.push(self.current_param);
                            let modes = std::mem::take(&mut self.params);
                            let raw_bytes = std::mem::take(&mut self.seq_bytes);
                            events.push(DecModeEvent::Reset { modes, raw_bytes });
                            self.state = ScanState::Ground;
                            self.current_param = 0;
                        }
                        _ => {
                            self.reset();
                        }
                    }
                }
            }
        }

        events
    }

    fn reset(&mut self) {
        self.state = ScanState::Ground;
        self.seq_bytes.clear();
        self.params.clear();
        self.current_param = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_alt_screen_enter() {
        let mut scanner = DecModeScanner::new();
        let events = scanner.scan(b"\x1b[?1049h");
        assert_eq!(events.len(), 1);
        assert!(events[0].enters_alt_screen());
        assert!(events[0].has_forwarded_mode());
        assert_eq!(events[0].raw_bytes(), b"\x1b[?1049h");
    }

    #[test]
    fn detects_alt_screen_exit() {
        let mut scanner = DecModeScanner::new();
        let events = scanner.scan(b"\x1b[?1049l");
        assert_eq!(events.len(), 1);
        assert!(events[0].exits_alt_screen());
        assert!(events[0].has_forwarded_mode());
    }

    #[test]
    fn detects_mouse_tracking() {
        let mut scanner = DecModeScanner::new();
        let events = scanner.scan(b"\x1b[?1000h");
        assert_eq!(events.len(), 1);
        assert!(events[0].has_forwarded_mode());
        assert!(!events[0].enters_alt_screen());
        match &events[0] {
            DecModeEvent::Set { modes, .. } => assert_eq!(modes, &[1000]),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn handles_compound_sequence() {
        let mut scanner = DecModeScanner::new();
        let events = scanner.scan(b"\x1b[?1049;1006h");
        assert_eq!(events.len(), 1);
        assert!(events[0].enters_alt_screen());
        assert!(events[0].has_forwarded_mode());
        match &events[0] {
            DecModeEvent::Set { modes, .. } => assert_eq!(modes, &[1049, 1006]),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn handles_split_across_buffers() {
        let mut scanner = DecModeScanner::new();

        let events1 = scanner.scan(b"\x1b[?10");
        assert!(events1.is_empty());

        let events2 = scanner.scan(b"49h");
        assert_eq!(events2.len(), 1);
        assert!(events2[0].enters_alt_screen());
        assert_eq!(events2[0].raw_bytes(), b"\x1b[?1049h");
    }

    #[test]
    fn handles_split_at_every_byte() {
        let mut scanner = DecModeScanner::new();
        let seq = b"\x1b[?1049h";
        for (i, &byte) in seq.iter().enumerate() {
            let events = scanner.scan(&[byte]);
            if i < seq.len() - 1 {
                assert!(events.is_empty());
            } else {
                assert_eq!(events.len(), 1);
                assert!(events[0].enters_alt_screen());
            }
        }
    }

    #[test]
    fn ignores_non_dec_csi() {
        let mut scanner = DecModeScanner::new();
        let events = scanner.scan(b"\x1b[1;31m");
        assert!(events.is_empty());
    }

    #[test]
    fn ignores_unknown_dec_modes() {
        let mut scanner = DecModeScanner::new();
        let events = scanner.scan(b"\x1b[?25l");
        assert_eq!(events.len(), 1);
        assert!(!events[0].has_forwarded_mode());
    }

    #[test]
    fn multiple_sequences_in_one_buffer() {
        let mut scanner = DecModeScanner::new();
        let events = scanner.scan(b"\x1b[?1049h\x1b[?1006h");
        assert_eq!(events.len(), 2);
        assert!(events[0].enters_alt_screen());
        match &events[1] {
            DecModeEvent::Set { modes, .. } => assert_eq!(modes, &[1006]),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn sequences_interleaved_with_text() {
        let mut scanner = DecModeScanner::new();
        let events = scanner.scan(b"hello\x1b[?1049hworld\x1b[?1049l");
        assert_eq!(events.len(), 2);
        assert!(events[0].enters_alt_screen());
        assert!(events[1].exits_alt_screen());
    }

    #[test]
    fn aborts_on_invalid_byte_in_params() {
        let mut scanner = DecModeScanner::new();
        let events = scanner.scan(b"\x1b[?1049x");
        assert!(events.is_empty());
        // Scanner should be back in ground state and able to parse next sequence
        let events = scanner.scan(b"\x1b[?1049h");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn mode_47_is_alt_screen() {
        let mut scanner = DecModeScanner::new();
        let events = scanner.scan(b"\x1b[?47h");
        assert_eq!(events.len(), 1);
        assert!(events[0].enters_alt_screen());
    }

    #[test]
    fn bracketed_paste_mode() {
        let mut scanner = DecModeScanner::new();
        let events = scanner.scan(b"\x1b[?2004h");
        assert_eq!(events.len(), 1);
        assert!(events[0].has_forwarded_mode());
        match &events[0] {
            DecModeEvent::Set { modes, .. } => assert_eq!(modes, &[2004]),
            _ => panic!("expected Set"),
        }
    }
}
