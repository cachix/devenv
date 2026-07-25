//! DEC private mode parser for `CSI ? <params> h/l` sequences.
//!
//! Handles parsing after the router has consumed `ESC [`.
//! Expects the first byte to be `?`, then digit/semicolon params,
//! terminated by `h` (set) or `l` (reset).

const MAX_PARAMS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecModeAction {
    Set { modes: Vec<u16> },
    Reset { modes: Vec<u16> },
}

pub enum DecModeResult {
    Pending,
    Complete(DecModeAction),
    /// Rejected: not a valid DEC private mode sequence.
    /// `private` is true if `?` was already consumed (i.e., the sequence
    /// started as DEC private but had an unexpected final byte).
    Reject {
        private: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    AwaitQuestion,
    Params,
}

pub struct DecModeParser {
    state: State,
    params: Vec<u16>,
    current_param: Option<u16>,
}

impl DecModeParser {
    pub fn new() -> Self {
        Self {
            state: State::AwaitQuestion,
            params: Vec::new(),
            current_param: None,
        }
    }

    pub fn feed(&mut self, byte: u8) -> DecModeResult {
        match self.state {
            State::AwaitQuestion => {
                if byte == b'?' {
                    self.state = State::Params;
                    self.params.clear();
                    self.current_param = Some(0);
                    DecModeResult::Pending
                } else {
                    DecModeResult::Reject { private: false }
                }
            }
            State::Params => match byte {
                b'0'..=b'9' => {
                    self.current_param = self
                        .current_param
                        .and_then(|v| v.checked_mul(10))
                        .and_then(|v| v.checked_add((byte - b'0') as u16));
                    DecModeResult::Pending
                }
                b';' => match self.current_param {
                    Some(val) if self.params.len() < MAX_PARAMS => {
                        self.params.push(val);
                        self.current_param = Some(0);
                        DecModeResult::Pending
                    }
                    _ => DecModeResult::Reject { private: true },
                },
                b'h' => self.finish(|modes| DecModeAction::Set { modes }),
                b'l' => self.finish(|modes| DecModeAction::Reset { modes }),
                _ => DecModeResult::Reject { private: true },
            },
        }
    }

    fn finish(&mut self, make_action: impl FnOnce(Vec<u16>) -> DecModeAction) -> DecModeResult {
        match self.current_param {
            Some(val) if self.params.len() < MAX_PARAMS => {
                self.params.push(val);
                let modes: Vec<u16> = std::mem::take(&mut self.params)
                    .into_iter()
                    .filter(|&m| m > 0)
                    .collect();
                self.current_param = Some(0);
                DecModeResult::Complete(make_action(modes))
            }
            _ => DecModeResult::Reject { private: true },
        }
    }

    pub fn reset(&mut self) {
        self.state = State::AwaitQuestion;
        self.params.clear();
        self.current_param = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_mark_transitions_to_params() {
        let mut p = DecModeParser::new();
        assert!(matches!(p.feed(b'?'), DecModeResult::Pending));
    }

    #[test]
    fn rejects_without_question_mark() {
        let mut p = DecModeParser::new();
        assert!(matches!(
            p.feed(b'1'),
            DecModeResult::Reject { private: false }
        ));
    }

    #[test]
    fn parses_set_mode() {
        let mut p = DecModeParser::new();
        p.feed(b'?');
        p.feed(b'1');
        p.feed(b'0');
        p.feed(b'4');
        p.feed(b'9');
        match p.feed(b'h') {
            DecModeResult::Complete(DecModeAction::Set { modes }) => {
                assert_eq!(modes, vec![1049]);
            }
            other => panic!(
                "expected Complete(Set), got {:?}",
                matches!(other, DecModeResult::Complete(_))
            ),
        }
    }

    #[test]
    fn parses_reset_mode() {
        let mut p = DecModeParser::new();
        p.feed(b'?');
        p.feed(b'1');
        p.feed(b'0');
        p.feed(b'4');
        p.feed(b'9');
        match p.feed(b'l') {
            DecModeResult::Complete(DecModeAction::Reset { modes }) => {
                assert_eq!(modes, vec![1049]);
            }
            other => panic!(
                "expected Complete(Reset), got {:?}",
                matches!(other, DecModeResult::Complete(_))
            ),
        }
    }

    #[test]
    fn parses_compound_params() {
        let mut p = DecModeParser::new();
        let mut result = None;
        for &b in b"?1049;1006h" {
            if let DecModeResult::Complete(action) = p.feed(b) {
                result = Some(action);
            }
        }
        match result {
            Some(DecModeAction::Set { modes }) => {
                assert_eq!(modes, vec![1049, 1006]);
            }
            _ => panic!("expected Complete(Set)"),
        }
    }

    #[test]
    fn compound_modes_parsed_correctly() {
        let mut p = DecModeParser::new();
        p.feed(b'?');
        p.feed(b'1');
        p.feed(b';');
        p.feed(b'2');
        match p.feed(b'h') {
            DecModeResult::Complete(DecModeAction::Set { modes }) => {
                assert_eq!(modes, vec![1, 2]);
            }
            _ => panic!("expected Complete(Set)"),
        }
    }

    #[test]
    fn rejects_invalid_byte_in_params() {
        let mut p = DecModeParser::new();
        p.feed(b'?');
        p.feed(b'1');
        assert!(matches!(
            p.feed(b'x'),
            DecModeResult::Reject { private: true }
        ));
    }

    #[test]
    fn rejects_overflowed_param() {
        let mut p = DecModeParser::new();
        p.feed(b'?');
        for &b in b"99999" {
            assert!(matches!(p.feed(b), DecModeResult::Pending));
        }
        assert!(matches!(
            p.feed(b'h'),
            DecModeResult::Reject { private: true }
        ));
    }

    #[test]
    fn rejects_too_many_params() {
        let mut p = DecModeParser::new();
        p.feed(b'?');
        for _ in 0..MAX_PARAMS {
            p.feed(b'1');
            assert!(matches!(p.feed(b';'), DecModeResult::Pending));
        }
        p.feed(b'1');
        assert!(matches!(
            p.feed(b';'),
            DecModeResult::Reject { private: true }
        ));
    }

    #[test]
    fn filters_zero_mode_params() {
        let mut p = DecModeParser::new();
        p.feed(b'?');
        p.feed(b';');
        for &b in b"1049" {
            p.feed(b);
        }
        match p.feed(b'h') {
            DecModeResult::Complete(DecModeAction::Set { modes }) => {
                assert_eq!(modes, vec![1049]);
            }
            _ => panic!("expected Complete(Set) without zero mode"),
        }
    }
}
