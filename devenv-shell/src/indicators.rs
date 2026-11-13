//! Status marks shown before the label in the shell status line.
//!
//! Renderers ask [`Indicators`] for a glyph instead of hard-coding one, so the
//! `tui.emoji` and `tui.symbols` user settings are applied in exactly one place.
//!
//! Emoji glyphs are not width-stable: terminals disagree on whether they occupy
//! one or two cells, and Unicode width tables report one. The non-emoji glyphs
//! are plain symbols that occupy a single cell, like the checkmark and spinner
//! already used by the status line, so the label that follows lines up with
//! every other status.

use std::collections::HashMap;

use crate::status_line::{CHECKMARK, DOT_RING, DOT_RUNNING, XMARK};

/// A status mark in the shell status line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Indicator {
    /// The last reload succeeded.
    Success,
    /// The last reload failed.
    Failure,
    /// File watching is paused.
    Paused,
    /// Files are being watched for changes.
    Watching,
}

impl Indicator {
    /// Every indicator.
    pub const ALL: [Indicator; 4] = [
        Indicator::Success,
        Indicator::Failure,
        Indicator::Paused,
        Indicator::Watching,
    ];
}

/// Selects the glyph used for each [`Indicator`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Indicators {
    emoji: bool,
    symbols: HashMap<Indicator, String>,
}

impl Indicators {
    /// Select emoji glyphs when `emoji` is true and non-emoji glyphs otherwise.
    pub fn new(emoji: bool) -> Self {
        Self {
            emoji,
            symbols: HashMap::new(),
        }
    }

    /// Draw `symbol` for `indicator` instead of its built-in glyph.
    pub fn with_symbol(mut self, indicator: Indicator, symbol: impl Into<String>) -> Self {
        self.symbols.insert(indicator, symbol.into());
        self
    }

    /// Whether emoji glyphs are selected.
    pub const fn emoji(&self) -> bool {
        self.emoji
    }

    /// The glyph for `indicator`.
    pub fn glyph(&self, indicator: Indicator) -> &str {
        self.symbols
            .get(&indicator)
            .map_or_else(|| self.built_in(indicator), String::as_str)
    }

    const fn built_in(&self, indicator: Indicator) -> &'static str {
        match (self.emoji, indicator) {
            (_, Indicator::Success) => CHECKMARK,
            (_, Indicator::Failure) => XMARK,
            // The historical glyphs.
            (true, Indicator::Paused) => "⏸",
            (true, Indicator::Watching) => "👁",
            // The status dots used for process states: alive while watching,
            // an idle ring while paused.
            (false, Indicator::Paused) => DOT_RING,
            (false, Indicator::Watching) => DOT_RUNNING,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_non_emoji() {
        assert_eq!(Indicators::default(), Indicators::new(false));
        assert!(!Indicators::default().emoji());
    }

    #[test]
    fn emoji_glyphs_are_the_historical_ones() {
        let indicators = Indicators::new(true);
        assert_eq!(indicators.glyph(Indicator::Paused), "⏸");
        assert_eq!(indicators.glyph(Indicator::Watching), "👁");
    }

    #[test]
    fn non_emoji_glyphs_are_single_characters() {
        for indicator in Indicator::ALL {
            assert_eq!(
                Indicators::new(false).glyph(indicator).chars().count(),
                1,
                "{indicator:?} must be one character"
            );
        }
    }

    #[test]
    fn only_watching_and_paused_differ_between_modes() {
        for indicator in Indicator::ALL {
            assert_eq!(
                Indicators::new(true).glyph(indicator) == Indicators::new(false).glyph(indicator),
                matches!(indicator, Indicator::Success | Indicator::Failure),
                "{indicator:?}"
            );
        }
    }

    #[test]
    fn symbols_replace_built_in_glyphs() {
        for emoji in [true, false] {
            let indicators = Indicators::new(emoji)
                .with_symbol(Indicator::Success, "ok")
                .with_symbol(Indicator::Watching, "*");

            assert_eq!(indicators.glyph(Indicator::Success), "ok");
            assert_eq!(indicators.glyph(Indicator::Watching), "*");
            assert_eq!(indicators.glyph(Indicator::Failure), XMARK);
            assert_eq!(
                indicators.glyph(Indicator::Paused),
                Indicators::new(emoji).glyph(Indicator::Paused)
            );
        }
    }
}
