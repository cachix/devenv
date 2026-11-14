//! Styling for log lines that contain ANSI escape sequences.
//!
//! [`parse`] splits a line into [`Span`]s, applying SGR colour/style codes and
//! dropping every other escape sequence. Control characters become spaces so
//! log content can never act as terminal input.

use anstyle_parse::{Params, Parser, Perform};
use iocraft::prelude::*;

/// Mirrors the styling fields of [`MixedTextContent`]. A `None` colour means
/// "use the caller's default".
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Style {
    pub color: Option<Color>,
    pub weight: Weight,
    pub decoration: TextDecoration,
    pub italic: bool,
    pub invert: bool,
}

/// A run of visible text sharing one style.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

impl Span {
    pub fn to_content(&self, default_color: Color) -> MixedTextContent {
        let mut content =
            MixedTextContent::new(&self.text).color(self.style.color.unwrap_or(default_color));
        content.weight = self.style.weight;
        content.decoration = self.style.decoration;
        content.italic = self.style.italic;
        content.invert = self.style.invert;
        content
    }
}

pub fn parse(line: &str) -> Vec<Span> {
    let mut parser = Parser::<anstyle_parse::DefaultCharAccumulator>::new();
    let mut performer = SpanBuilder::default();
    for byte in line.bytes() {
        parser.advance(&mut performer, byte);
    }
    performer.spans
}

/// The visible text of `line`, without escape sequences.
pub fn strip(line: &str) -> String {
    text(&parse(line))
}

pub fn text(spans: &[Span]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

/// The parts of `spans` covering visible characters `start..end`.
pub fn slice(spans: &[Span], start: usize, end: usize) -> Vec<Span> {
    let mut offset = 0;
    spans
        .iter()
        .filter_map(|span| {
            let len = span.text.chars().count();
            let from = start.max(offset) - offset;
            let to = end.min(offset + len).saturating_sub(offset);
            offset += len;
            (to > from).then(|| Span {
                text: span.text.chars().skip(from).take(to - from).collect(),
                style: span.style,
            })
        })
        .collect()
}

#[derive(Default)]
struct SpanBuilder {
    spans: Vec<Span>,
    style: Style,
}

impl SpanBuilder {
    fn push(&mut self, character: char) {
        match self.spans.last_mut() {
            Some(span) if span.style == self.style => span.text.push(character),
            _ => self.spans.push(Span {
                text: character.to_string(),
                style: self.style,
            }),
        }
    }
}

impl Perform for SpanBuilder {
    fn print(&mut self, character: char) {
        // C1 controls and DEL arrive here; keep them out of the terminal too.
        self.push(if character.is_control() {
            ' '
        } else {
            character
        });
    }

    fn execute(&mut self, _byte: u8) {
        self.push(' ');
    }

    fn csi_dispatch(&mut self, params: &Params, _: &[u8], _: bool, action: u8) {
        if action == b'm' {
            apply_sgr(&mut self.style, params);
        }
    }
}

fn apply_sgr(style: &mut Style, params: &Params) {
    let mut groups = params.iter();
    while let Some(group) = groups.next() {
        let Some(&code) = group.first() else { continue };
        match code {
            0 => *style = Style::default(),
            1 => style.weight = Weight::Bold,
            2 => style.weight = Weight::Light,
            22 => style.weight = Weight::Normal,
            3 | 23 => style.italic = code == 3,
            4 | 24 => {
                style.decoration = if code == 4 {
                    TextDecoration::Underline
                } else {
                    TextDecoration::None
                }
            }
            7 | 27 => style.invert = code == 7,
            30..=37 => style.color = Some(Color::AnsiValue((code - 30) as u8)),
            90..=97 => style.color = Some(Color::AnsiValue((code - 90 + 8) as u8)),
            38 => style.color = extended_color(group, &mut groups),
            39 => style.color = None,
            // Backgrounds are not rendered, but their arguments must be consumed.
            48 => _ = extended_color(group, &mut groups),
            _ => {}
        }
    }
}

/// An extended colour: `5;n` or `2;r;g;b`. The colon form carries its
/// arguments in `group`, optionally with a colour space id (`2::r:g:b`);
/// the semicolon form takes them from the following groups.
fn extended_color<'a>(
    group: &[u16],
    groups: &mut impl Iterator<Item = &'a [u16]>,
) -> Option<Color> {
    let mut args = if group.len() > 1 {
        group[1..].to_vec()
    } else {
        let mode = *groups.next()?.first()?;
        let count = match mode {
            5 => 1,
            2 => 3,
            _ => 0,
        };
        let rest = groups
            .by_ref()
            .take(count)
            .filter_map(|group| group.first().copied());
        std::iter::once(mode).chain(rest).collect()
    };
    if args.len() == 5 && args[0] == 2 {
        args.remove(1);
    }
    let arg = |index: usize| args.get(index).map(|&value| value.min(255) as u8);
    match args.first()? {
        5 => Some(Color::AnsiValue(arg(1)?)),
        2 => Some(Color::Rgb {
            r: arg(1)?,
            g: arg(2)?,
            b: arg(3)?,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors(line: &str) -> Vec<Option<Color>> {
        colors_of(&parse(line))
    }

    fn colors_of(spans: &[Span]) -> Vec<Option<Color>> {
        spans.iter().map(|span| span.style.color).collect()
    }

    #[test]
    fn colours() {
        assert_eq!(
            colors("\x1b[31ma\x1b[0mb\x1b[91mc\x1b[mx"),
            [
                Some(Color::AnsiValue(1)),
                None,
                Some(Color::AnsiValue(9)),
                None
            ]
        );
        assert_eq!(
            colors("\x1b[38;5;208ma\x1b[38;2;1;2;3mb\x1b[38:2:4:5:6mc\x1b[38:2::7:8:9md"),
            [
                Some(Color::AnsiValue(208)),
                Some(Color::Rgb { r: 1, g: 2, b: 3 }),
                Some(Color::Rgb { r: 4, g: 5, b: 6 }),
                Some(Color::Rgb { r: 7, g: 8, b: 9 }),
            ]
        );
        // `5;1` belongs to the background, so it must not set bold.
        let spans = parse("\x1b[48;5;1;31mx");
        assert_eq!(spans[0].style.color, Some(Color::AnsiValue(1)));
        assert_eq!(spans[0].style.weight, Weight::Normal);
    }

    #[test]
    fn attributes() {
        let spans = parse("\x1b[1;4mx\x1b[22my\x1b[24mz");
        let styles: Vec<_> = spans
            .iter()
            .map(|span| (span.style.weight, span.style.decoration))
            .collect();
        assert_eq!(
            styles,
            [
                (Weight::Bold, TextDecoration::Underline),
                (Weight::Normal, TextDecoration::Underline),
                (Weight::Normal, TextDecoration::None),
            ]
        );
    }

    #[test]
    fn strips_other_sequences_and_controls() {
        assert_eq!(strip("\x1b[2K\x1b[1Gprogress"), "progress");
        assert_eq!(
            strip("\x1b]8;;https://devenv.sh\x1b\\link\x1b]8;;\x07!"),
            "link!"
        );
        assert_eq!(strip("a\tb\x07c\u{9b}d\x7fe"), "a b c d e");
        assert_eq!(strip("tail\x1b["), "tail");
    }

    #[test]
    fn slices_by_visible_characters() {
        let spans = parse("ab\x1b[31mcd\x1b[0mef");
        let sliced = slice(&spans, 1, 5);
        assert_eq!(text(&sliced), "bcde");
        assert_eq!(colors_of(&sliced), [None, Some(Color::AnsiValue(1)), None]);
    }
}
