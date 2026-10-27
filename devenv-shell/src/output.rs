//! Redact PTY text using the same Ghostty stream that renders the session.

use devenv_mailbox::ShellOutputFilter;
use libghostty_vt::terminal::{Boundary, Terminal};

const STRING_END: Boundary = Boundary::OSC_END
    .union(Boundary::DCS_END)
    .union(Boundary::APC_END);
const STRING_BOUNDARIES: Boundary = STRING_END
    .union(Boundary::OSC_START)
    .union(Boundary::DCS_START)
    .union(Boundary::APC_START);

/// Payload policy for a string Ghostty has entered. Ghostty owns the
/// transitions into and out of these strings, including aborts.
#[derive(Clone, Copy)]
enum Payload {
    OscCommand(u16),
    HyperlinkParams,
    DcsHeader { first_dollar: Option<bool> },
    Apc,
    Text,
    Syntax,
}

pub(crate) struct TerminalOutputFilter {
    filter: Box<dyn ShellOutputFilter>,
    payload: Option<Payload>,
    // Render only complete UTF-8 while matching ground text. Otherwise a
    // write split in a character would make Ghostty report non-ground.
    text: Vec<u8>,
    // The legacy screen title (ESC k ... ST) is forwarded to the host but
    // has no equivalent string state in Ghostty.
    after_escape: bool,
    screen_title: bool,
}

impl TerminalOutputFilter {
    pub(crate) fn new(filter: Box<dyn ShellOutputFilter>) -> Self {
        Self {
            filter,
            payload: None,
            text: Vec::new(),
            after_escape: false,
            screen_title: false,
        }
    }

    /// Apply the filtered bytes to Ghostty and append the sanitized stream
    /// for routing any host terminal controls.
    pub(crate) fn push(
        &mut self,
        terminal: &mut Terminal<'_, '_>,
        mut input: &[u8],
        output: &mut Vec<u8>,
    ) -> libghostty_vt::error::Result<()> {
        while let Some((&byte, _)) = input.split_first() {
            let count = if byte == 0x1b {
                self.end_region(terminal, output);
                self.screen_title = false;
                self.write_control(terminal, &input[..1], false, output)?;
                self.after_escape = true;
                1
            } else if std::mem::take(&mut self.after_escape) && byte == b'k' {
                // Ghostty has consumed ESC. Complete it with a no-op in
                // place of the unsupported title opener.
                terminal.vt_write(b"\\");
                self.payload = None;
                self.screen_title = true;
                output.push(byte);
                1
            } else if let Some(payload) = self.payload {
                self.push_payload(terminal, payload, input, output)?
            } else if self.screen_title || terminal.is_vt_ground()? {
                let count = prefix_len(input, |b| b == 0x1b);
                self.push_text(terminal, &input[..count], output);
                count
            } else {
                let count = prefix_len(input, |b| b == 0x1b);
                self.write_control(terminal, &input[..count], true, output)?
            };
            input = &input[count..];
        }
        Ok(())
    }

    pub(crate) fn end_region(&mut self, terminal: &mut Terminal<'_, '_>, output: &mut Vec<u8>) {
        self.filter.end_region(&mut self.text);
        self.emit_text(terminal, true, output);
    }

    fn push_text(&mut self, terminal: &mut Terminal<'_, '_>, input: &[u8], output: &mut Vec<u8>) {
        self.filter.push(input, &mut self.text);
        self.emit_text(terminal, false, output);
    }

    fn emit_text(&mut self, terminal: &mut Terminal<'_, '_>, end: bool, output: &mut Vec<u8>) {
        let keep = if !end && self.payload.is_none() && !self.screen_title {
            self.text
                .utf8_chunks()
                .last()
                .map(|chunk| chunk.invalid())
                .filter(|tail| std::str::from_utf8(tail).is_err_and(|e| e.error_len().is_none()))
                .map_or(0, <[u8]>::len)
        } else {
            0
        };
        let emitted = self.text.len() - keep;
        if !self.screen_title && emitted != 0 {
            terminal.vt_write(&self.text[..emitted]);
        }
        output.extend_from_slice(&self.text[..emitted]);
        self.text.copy_within(emitted.., 0);
        self.text.truncate(keep);
    }

    fn write_control(
        &mut self,
        terminal: &mut Terminal<'_, '_>,
        input: &[u8],
        until_ground: bool,
        output: &mut Vec<u8>,
    ) -> libghostty_vt::error::Result<usize> {
        let boundaries = if until_ground {
            STRING_BOUNDARIES | Boundary::GROUND
        } else {
            STRING_BOUNDARIES
        };
        let Some(stop) = terminal.vt_write_until(input, boundaries)? else {
            output.extend_from_slice(input);
            return Ok(input.len());
        };

        if stop.reached.intersects(STRING_END) {
            self.payload = None;
        }
        if stop.reached.contains(Boundary::OSC_START) {
            self.payload = Some(Payload::OscCommand(0));
        } else if stop.reached.contains(Boundary::DCS_START) {
            self.payload = Some(Payload::DcsHeader { first_dollar: None });
        } else if stop.reached.contains(Boundary::APC_START) {
            self.payload = Some(Payload::Apc);
        }
        output.extend_from_slice(&input[..stop.consumed]);
        Ok(stop.consumed)
    }

    fn push_payload(
        &mut self,
        terminal: &mut Terminal<'_, '_>,
        payload: Payload,
        input: &[u8],
        output: &mut Vec<u8>,
    ) -> libghostty_vt::error::Result<usize> {
        let byte = input[0];
        let control = |b| {
            b < 0x20 || b == 0x7f || matches!(payload, Payload::Apc) && (0x80..=0x9f).contains(&b)
        };
        if control(byte) {
            self.end_region(terminal, output);
            return self.write_control(terminal, &input[..1], false, output);
        }

        match payload {
            Payload::OscCommand(command) => match byte {
                b'0'..=b'9' => {
                    self.payload = Some(Payload::OscCommand(
                        command
                            .saturating_mul(10)
                            .saturating_add(u16::from(byte - b'0')),
                    ));
                }
                b';' => {
                    self.payload = Some(match command {
                        8 => Payload::HyperlinkParams,
                        4..=6 | 10..=19 | 104..=106 | 110..=119 => Payload::Syntax,
                        _ => Payload::Text,
                    });
                }
                _ => {
                    self.payload = Some(Payload::Text);
                    return self.push_payload(terminal, Payload::Text, input, output);
                }
            },
            Payload::DcsHeader { first_dollar } => {
                self.payload = Some(match byte {
                    0x40..=0x7e if first_dollar == Some(true) && byte == b'q' => Payload::Syntax,
                    0x40..=0x7e => Payload::Text,
                    0x90 => Payload::DcsHeader { first_dollar: None },
                    0x20..=0x3f => Payload::DcsHeader {
                        first_dollar: Some(first_dollar.is_none() && byte == b'$'),
                    },
                    _ => payload,
                });
            }
            Payload::HyperlinkParams if byte == b';' => {
                self.end_region(terminal, output);
                self.payload = Some(Payload::Text);
            }
            Payload::Syntax => {
                let count = prefix_len(input, control);
                return self.write_control(terminal, &input[..count], false, output);
            }
            _ => {
                let count = prefix_len(input, |b| {
                    control(b) || matches!(payload, Payload::HyperlinkParams) && b == b';'
                });
                self.push_text(terminal, &input[..count], output);
                return Ok(count);
            }
        }

        self.write_control(terminal, &input[..1], false, output)
    }
}

fn prefix_len(input: &[u8], stop: impl FnMut(u8) -> bool) -> usize {
    input.iter().copied().position(stop).unwrap_or(input.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vt_utils::{active_point, row_plain_text};

    #[derive(Debug, Default)]
    struct RedactToken(Vec<u8>);

    impl ShellOutputFilter for RedactToken {
        fn push(&mut self, input: &[u8], _: &mut Vec<u8>) {
            self.0.extend_from_slice(input);
        }

        fn end_region(&mut self, output: &mut Vec<u8>) {
            // This fixture's matcher buffers entire regions so a change in
            // read boundaries cannot hide an incorrectly split text region.
            let mut text = self.0.as_slice();
            while let Some(offset) = text.windows(5).position(|part| part == b"token") {
                output.extend_from_slice(&text[..offset]);
                output.extend_from_slice(b"[MASKED]");
                text = &text[offset + 5..];
            }
            output.extend_from_slice(text);
            self.0.clear();
        }
    }

    #[test]
    fn ghostty_boundaries_preserve_syntax_and_redact_before_terminal_mutation() {
        for (input, expected) in [
            (
                b"token\x1b[?2004h\x1b]0;token\x07\x1b]8;id=token;https://example.test/token\x1b\\\x1b]10;?\x1b\\\x1bP$qm\x1b\\\x1bP1qtoken\x1b\\\x1b_token\x1b\\token".as_slice(),
                b"[MASKED]\x1b[?2004h\x1b]0;[MASKED]\x07\x1b]8;id=[MASKED];https://example.test/[MASKED]\x1b\\\x1b]10;?\x1b\\\x1bP$qm\x1b\\\x1bP1q[MASKED]\x1b\\\x1b_[MASKED]\x1b\\[MASKED]".as_slice(),
            ),
            (
                b"\x1bkterminal token\x1b\\\x1b]0;abandoned token\x18\x1b]0;token\x1b\\token".as_slice(),
                b"\x1bkterminal [MASKED]\x1b\\\x1b]0;abandoned [MASKED]\x18\x1b]0;[MASKED]\x1b\\[MASKED]".as_slice(),
            ),
        ] {
            for split in 0..=input.len() {
                let mut vt = Terminal::new(80, 24).unwrap();
                let mut filter = TerminalOutputFilter::new(Box::<RedactToken>::default());
                let mut output = Vec::new();
                filter.push(&mut vt, &input[..split], &mut output).unwrap();
                filter.push(&mut vt, &input[split..], &mut output).unwrap();
                filter.end_region(&mut vt, &mut output);
                assert_eq!(output, expected, "split at {split}");
                assert_eq!(vt.title().unwrap(), b"[MASKED]");
                let row = row_plain_text(&vt, active_point(0));
                assert_eq!(row.trim(), if input[0] == b't' { "[MASKED][MASKED]" } else { "[MASKED]" });
            }
        }
    }

    #[test]
    fn a_split_utf8_character_does_not_put_the_terminal_in_control_state() {
        #[derive(Debug)]
        struct Copy;
        impl ShellOutputFilter for Copy {
            fn push(&mut self, input: &[u8], output: &mut Vec<u8>) {
                output.extend_from_slice(input);
            }
            fn end_region(&mut self, _: &mut Vec<u8>) {}
        }

        let mut vt = Terminal::new(80, 24).unwrap();
        let mut filter = TerminalOutputFilter::new(Box::new(Copy));
        let mut output = Vec::new();
        filter.push(&mut vt, b"\xf0\x9f", &mut output).unwrap();
        assert!(vt.is_vt_ground().unwrap());
        assert!(output.is_empty());
        filter.push(&mut vt, b"\x8c\xb1", &mut output).unwrap();
        assert_eq!(output, "🌱".as_bytes());
        assert_eq!(row_plain_text(&vt, active_point(0)).trim(), "🌱");
    }
}
