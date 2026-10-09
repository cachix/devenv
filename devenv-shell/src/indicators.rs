//! Display indicators that have an emoji and a non-emoji form.
//!
//! Renderers ask [`Indicators`] for a glyph instead of hard-coding one, so the
//! `tui.emoji` user setting is applied in exactly one place.
//!
//! Emoji glyphs are not width-stable: terminals disagree on whether they occupy
//! one or two cells, and Unicode width tables report one. The non-emoji glyphs
//! are plain symbols that occupy a single cell, like the checkmark and spinner
//! already used by the status line, so the label that follows lines up with
//! every other status.

use crate::status_line::{DOT_RING, DOT_RUNNING};

/// A display indicator with an emoji and a non-emoji form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indicator {
    /// File watching is paused.
    Paused,
    /// Files are being watched for changes.
    Watching,
}

impl Indicator {
    /// Every indicator.
    pub const ALL: [Indicator; 2] = [Indicator::Paused, Indicator::Watching];
}

/// A glyph and the space that separates it from the label that follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    /// The text to draw.
    pub text: &'static str,
    /// Columns between the glyph and its label.
    pub gap: u32,
}

/// Selects the glyph used for each [`Indicator`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Indicators {
    emoji: bool,
}

impl Indicators {
    /// Select emoji glyphs when `emoji` is true and non-emoji glyphs otherwise.
    pub const fn new(emoji: bool) -> Self {
        Self { emoji }
    }

    /// Whether emoji glyphs are selected.
    pub const fn emoji(self) -> bool {
        self.emoji
    }

    /// The glyph for `indicator`.
    pub const fn glyph(self, indicator: Indicator) -> Glyph {
        match (self.emoji, indicator) {
            // The historical glyphs and spacing.
            (true, Indicator::Paused) => Glyph {
                text: "⏸", gap: 2
            },
            (true, Indicator::Watching) => Glyph {
                text: "👁", gap: 2
            },
            // The status dots used for process states: alive while watching,
            // an idle ring while paused. They are single-cell symbols, so the
            // label starts in the same column as after the checkmark, cross,
            // and spinner.
            (false, Indicator::Paused) => Glyph {
                text: DOT_RING,
                gap: 1,
            },
            (false, Indicator::Watching) => Glyph {
                text: DOT_RUNNING,
                gap: 1,
            },
        }
    }
}

impl Default for Indicators {
    /// Emoji glyphs, preserving the existing output.
    fn default() -> Self {
        Self::new(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_emoji() {
        assert_eq!(Indicators::default(), Indicators::new(true));
        assert!(Indicators::default().emoji());
    }

    #[test]
    fn emoji_glyphs_are_the_historical_ones() {
        let indicators = Indicators::new(true);
        assert_eq!(indicators.glyph(Indicator::Paused).text, "⏸");
        assert_eq!(indicators.glyph(Indicator::Watching).text, "👁");
    }

    #[test]
    fn non_emoji_glyphs_are_single_non_emoji_characters() {
        for indicator in Indicator::ALL {
            let glyph = Indicators::new(false).glyph(indicator);
            let mut chars = glyph.text.chars();
            assert!(chars.next().is_some());
            assert!(
                chars.next().is_none(),
                "{indicator:?} must be one character"
            );
            assert_eq!(glyph.gap, 1);
        }
    }

    #[test]
    fn glyphs_differ_between_modes() {
        for indicator in Indicator::ALL {
            assert_ne!(
                Indicators::new(true).glyph(indicator),
                Indicators::new(false).glyph(indicator)
            );
        }
    }
}
