//! OSC (Operating System Command) sequence parser.
//!
//! Handles parsing after the router has consumed `ESC ]`.
//! Accumulates payload bytes until BEL (0x07) terminates.
//! ST (ESC \) termination is handled by the router.
//! Produces events for any well-formed OSC sequence so terminal features like
//! hyperlinks and title updates survive the PTY renderer.

use std::io::{self, Write};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OscEvent {
    pub raw_bytes: Vec<u8>,
}

impl OscEvent {
    pub(crate) fn write_to_outer_terminal(&self, stdout: &mut impl Write) -> io::Result<()> {
        let Some((osc, b"")) = parse_default_color_osc(&self.raw_bytes) else {
            return stdout.write_all(&self.raw_bytes);
        };
        let operations = (osc.code..=19).zip(osc.value.split(|byte| *byte == b';'));
        let is_virtual_query = |(code, value): (u8, &[u8])| code <= 12 && value == b"?";
        if !operations.clone().any(is_virtual_query) {
            return stdout.write_all(&self.raw_bytes);
        }

        // A dynamic-color OSC can set one color and query another. Let
        // libghostty answer OSC 10–12 queries while the outer terminal receives
        // the remaining operations with their original color numbers.
        for (code, value) in operations {
            if value.is_empty() || is_virtual_query((code, value)) {
                continue;
            }
            write!(stdout, "\x1b]{code};")?;
            stdout.write_all(value)?;
            stdout.write_all(osc.terminator)?;
        }
        Ok(())
    }
}

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
    fn forward_osc_without_virtual_color_queries() {
        let mut stdout = Vec::new();
        for bytes in [
            b"\x1b]0;window title\x07".as_slice(),
            b"\x1b]11;?\x1b\\",
            b"\x1b]10;?;rgb:12/34/56;?;?\x07",
            b"\x1b]10;?;;blue\x1b\\",
            b"\x1b]111\x07",
            b"\x1b]4;0;?\x1b\\",
        ] {
            OscEvent {
                raw_bytes: bytes.to_vec(),
            }
            .write_to_outer_terminal(&mut stdout)
            .unwrap();
        }
        assert_eq!(
            stdout,
            b"\x1b]0;window title\x07\x1b]11;rgb:12/34/56\x07\x1b]13;?\x07\x1b]12;blue\x1b\\\x1b]111\x07\x1b]4;0;?\x1b\\"
        );
    }

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
