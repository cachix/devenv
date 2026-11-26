//! OSC (Operating System Command) sequence parser.
//!
//! Handles parsing after the router has consumed `ESC ]`.
//! Accumulates payload bytes until BEL (0x07) terminates.
//! ST (ESC \) termination is handled by the router.
//! Produces events for any well-formed OSC sequence so terminal features like
//! hyperlinks and title updates survive the PTY renderer.

pub(crate) struct DefaultColorOsc<'a> {
    pub code: u8,
    pub value: &'a [u8],
    pub terminator: &'a [u8],
}

/// Parse one OSC 10, 11 or 12 sequence at the start of a byte stream.
pub(crate) fn parse_default_color_osc(input: &[u8]) -> Option<(DefaultColorOsc<'_>, &[u8])> {
    let [0x1b, b']', b'1', code @ b'0'..=b'2', b';', data @ ..] = input else {
        return None;
    };
    let end = data.iter().position(|byte| byte.is_ascii_control())?;
    let (value, tail) = data.split_at(end);
    let (terminator, rest) = match tail {
        [0x07, rest @ ..] => (&tail[..1], rest),
        [0x1b, b'\\', rest @ ..] => (&tail[..2], rest),
        _ => return None,
    };
    Some((
        DefaultColorOsc {
            code: 10 + (*code - b'0'),
            value,
            terminator,
        },
        rest,
    ))
}

pub enum OscResult {
    Pending,
    Complete,
    Reject,
}

pub struct OscParser {
    payload: Vec<u8>,
}

impl OscParser {
    pub fn new() -> Self {
        Self {
            payload: Vec::with_capacity(128),
        }
    }

    pub fn feed(&mut self, byte: u8) -> OscResult {
        match byte {
            0x07 => self.finish(),
            0x00..=0x06 | 0x08..=0x1f | 0x7f => OscResult::Reject,
            _ => {
                self.payload.push(byte);
                OscResult::Pending
            }
        }
    }

    pub fn finish(&self) -> OscResult {
        OscResult::Complete
    }

    pub fn reset(&mut self) {
        self.payload.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_with_bel() {
        let mut p = OscParser::new();
        for &b in b"11;?" {
            assert!(matches!(p.feed(b), OscResult::Pending));
        }
        assert!(matches!(p.feed(0x07), OscResult::Complete));
    }

    #[test]
    fn non_query_completes() {
        let mut p = OscParser::new();
        for &b in b"0;title" {
            assert!(matches!(p.feed(b), OscResult::Pending));
        }
        assert!(matches!(p.feed(0x07), OscResult::Complete));
    }

    #[test]
    fn c0_control_rejects() {
        let mut p = OscParser::new();
        p.feed(b'1');
        assert!(matches!(p.feed(0x00), OscResult::Reject));
    }

    #[test]
    fn del_rejects() {
        let mut p = OscParser::new();
        p.feed(b'1');
        assert!(matches!(p.feed(0x7f), OscResult::Reject));
    }

    #[test]
    fn empty_payload_query_with_just_question_mark() {
        let mut p = OscParser::new();
        p.feed(b'?');
        assert!(matches!(p.feed(0x07), OscResult::Complete));
    }

    #[test]
    fn finish_checks_payload() {
        let mut p = OscParser::new();
        for &b in b"11;?" {
            p.feed(b);
        }
        assert!(matches!(p.finish(), OscResult::Complete));
    }

    #[test]
    fn finish_completes_non_query() {
        let mut p = OscParser::new();
        for &b in b"0;title" {
            p.feed(b);
        }
        assert!(matches!(p.finish(), OscResult::Complete));
    }
}
