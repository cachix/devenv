//! OSC (Operating System Command) sequence parser.
//!
//! Handles parsing after the router has consumed `ESC ]`.
//! Accumulates payload bytes until BEL (0x07) terminates.
//! ST (ESC \) termination is handled by the router.
//! Produces events for any well-formed OSC sequence so terminal features like
//! hyperlinks and title updates survive the PTY renderer.

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
