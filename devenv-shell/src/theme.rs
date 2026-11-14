use crate::status_line::{
    COLOR_ACTIVE, COLOR_COMPLETED, COLOR_FAILED, COLOR_INTERACTIVE, COLOR_SECONDARY,
};
use crossterm::style::{Attribute, Attributes, ContentStyle};
use iocraft::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
}

impl Style {
    pub fn foreground(color: Color) -> Self {
        Self {
            foreground: Some(color),
            ..Self::default()
        }
    }

    pub fn colors(foreground: Color, background: Color) -> Self {
        Self {
            foreground: Some(foreground),
            background: Some(background),
            ..Self::default()
        }
    }

    pub fn or_reversed(self) -> Self {
        Self {
            reverse: self.reverse || self.foreground.is_none() && self.background.is_none(),
            ..self
        }
    }

    pub fn emphasized(self) -> Self {
        Self {
            bold: self.bold || !self.dim,
            ..self
        }
    }

    pub fn layered(self, over: Style) -> Style {
        Style {
            foreground: over.foreground.or(self.foreground),
            background: over.background.or(self.background),
            bold: over.bold || self.bold && !over.dim,
            dim: over.dim || self.dim && !over.bold,
            italic: self.italic || over.italic,
            underline: self.underline || over.underline,
            reverse: self.reverse || over.reverse,
        }
    }

    pub fn text(&self, content: impl Into<String>) -> Element<'static, Text> {
        element!(Text(
            content: content.into(),
            color: self.foreground,
            weight: if self.bold { Weight::Bold } else if self.dim { Weight::Light } else { Weight::Normal },
            decoration: if self.underline { TextDecoration::Underline } else { TextDecoration::None },
            italic: self.italic,
            invert: self.reverse,
        ))
    }

    pub fn content_style(&self) -> ContentStyle {
        let mut attributes = Attributes::default();
        for (enabled, attribute) in [
            (self.bold, Attribute::Bold),
            (self.dim && !self.bold, Attribute::Dim),
            (self.italic, Attribute::Italic),
            (self.underline, Attribute::Underlined),
            (self.reverse, Attribute::Reverse),
        ] {
            if enabled {
                attributes.set(attribute);
            }
        }
        ContentStyle {
            foreground_color: self.foreground,
            background_color: self.background,
            underline_color: None,
            attributes,
        }
    }

    pub fn span(&self, content: impl Into<String>) -> AnyElement<'static> {
        match self.background {
            Some(background) => element!(View(background_color: background) {
                #(self.text(content))
            })
            .into_any(),
            None => self.text(content).into_any(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellTheme {
    pub text: Style,
    pub label: Style,
    pub value: Style,
    pub key: Style,
    pub active: Style,
    pub success: Style,
    pub failure: Style,
    pub state: Style,
    pub state_success: Style,
    pub state_failure: Style,
}

impl Default for ShellTheme {
    fn default() -> Self {
        let state = Style {
            bold: true,
            ..Style::foreground(COLOR_ACTIVE)
        };
        Self {
            text: Style::default(),
            label: Style::foreground(COLOR_SECONDARY),
            value: Style::foreground(COLOR_COMPLETED),
            key: Style::foreground(COLOR_INTERACTIVE),
            active: Style::foreground(COLOR_ACTIVE),
            success: Style::foreground(COLOR_COMPLETED),
            failure: Style::foreground(COLOR_FAILED),
            state,
            state_success: state.layered(Style::foreground(COLOR_COMPLETED)),
            state_failure: state.layered(Style::foreground(COLOR_FAILED)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_style_matches_drawn_text() {
        let style = Style {
            bold: true,
            dim: true,
            underline: true,
            ..Style::colors(Color::Red, Color::Blue)
        };
        let content = style.content_style();

        assert_eq!(content.foreground_color, Some(Color::Red));
        assert_eq!(content.background_color, Some(Color::Blue));
        assert!(content.attributes.has(Attribute::Bold));
        assert!(!content.attributes.has(Attribute::Dim));
        assert!(content.attributes.has(Attribute::Underlined));
        assert_eq!(Style::default().content_style(), ContentStyle::default());
    }
}
