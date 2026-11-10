use crate::{
    components::{
        COLOR_ACTIVE, COLOR_ACTIVE_NESTED, COLOR_COMPLETED, COLOR_FAILED, COLOR_HIERARCHY,
        COLOR_INFO, COLOR_INTERACTIVE, COLOR_SECONDARY, COLOR_TRANSIENT,
    },
    config::{StyleConfig, TextModifier, ThemeConfig, ThemePreset},
};
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
    fn foreground(color: Color) -> Self {
        Self {
            foreground: Some(color),
            ..Self::default()
        }
    }

    fn colors(foreground: Color, background: Color) -> Self {
        Self {
            foreground: Some(foreground),
            background: Some(background),
            ..Self::default()
        }
    }

    fn or_reversed(self) -> Self {
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

impl ThemeConfig {
    pub fn style(&self, scopes: &[&str]) -> Style {
        scopes
            .iter()
            .map(|scope| preset_style(self.preset, scope))
            .fold(Style::default(), Style::layered)
            .layered(self.user_style(scopes))
    }

    pub fn user_style(&self, scopes: &[&str]) -> Style {
        scopes
            .iter()
            .filter_map(|scope| self.styles.get(*scope))
            .map(|style| self.configured_style(style))
            .fold(Style::default(), Style::layered)
    }

    pub fn configured_style(&self, style: &StyleConfig) -> Style {
        let color = |value: Option<&_>| value.and_then(|value| self.resolve_color(value).ok());
        let has = |modifier| style.modifiers.contains(&modifier);
        Style {
            foreground: color(style.foreground.as_ref()),
            background: color(style.background.as_ref()),
            bold: has(TextModifier::Bold),
            dim: has(TextModifier::Dim),
            italic: has(TextModifier::Italic),
            underline: has(TextModifier::Underline),
            reverse: has(TextModifier::Reverse),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Active,
    Success,
    Failure,
    Warning,
    Info,
    Starting,
    Inactive,
}

impl Status {
    pub fn of_completion(completed: Option<bool>) -> Self {
        match completed {
            None => Self::Active,
            Some(true) => Self::Success,
            Some(false) => Self::Failure,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Highlight {
    None,
    Selected,
    Dimmed,
}

impl Highlight {
    pub fn new(is_selected: bool, is_dimmed: bool) -> Self {
        if is_selected {
            Self::Selected
        } else if is_dimmed {
            Self::Dimmed
        } else {
            Self::None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Message,
    Name,
    NestedName,
    Detail,
    Elapsed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityTheme {
    pub detail: Style,
    pub tree: Style,
    pub log: Style,
    pub progress: Style,
    pub progress_track: Style,
    message: Style,
    name: Style,
    nested_name: Style,
    elapsed: Style,
    selection: Style,
    selected_name: Style,
    selected_detail: Style,
    selected_elapsed: Style,
    dimmed: Style,
    active: Style,
    success: Style,
    failure: Style,
    warning: Style,
    info: Style,
    starting: Style,
    inactive: Style,
}

impl ActivityTheme {
    pub fn new(theme: &ThemeConfig) -> Self {
        let status = |scope| theme.style(&["activity", "activity.status", scope]);
        let selection_style = |scopes: &[&str]| {
            if theme.styles.contains_key("activity.selection") {
                theme.user_style(scopes)
            } else {
                theme.style(scopes)
            }
        };
        let selection = selection_style(&["activity.selection"]);
        let selected = |scope| selection_style(&["activity.selection", scope]);
        Self {
            detail: theme.style(&["activity", "activity.detail"]),
            tree: theme.style(&["activity", "activity.tree"]),
            log: theme.style(&["activity", "activity.log"]),
            progress: theme.style(&["activity", "activity.progress"]),
            progress_track: theme.style(&[
                "activity",
                "activity.progress",
                "activity.progress.track",
            ]),
            message: theme.style(&["activity"]),
            name: theme.style(&["activity", "activity.name"]),
            nested_name: theme.style(&["activity", "activity.name", "activity.name.nested"]),
            elapsed: theme.style(&["activity", "activity.elapsed"]),
            selection,
            selected_name: selected("activity.selection.name"),
            selected_detail: selected("activity.selection.detail"),
            selected_elapsed: selected("activity.selection.elapsed"),
            dimmed: theme.style(&["activity.dimmed"]),
            active: status("activity.status.active"),
            success: status("activity.status.success"),
            failure: status("activity.status.failure"),
            warning: status("activity.status.warning"),
            info: status("activity.status.info"),
            starting: status("activity.status.starting"),
            inactive: status("activity.status.inactive"),
        }
    }

    pub fn status(&self, status: Status) -> Style {
        match status {
            Status::Active => self.active,
            Status::Success => self.success,
            Status::Failure => self.failure,
            Status::Warning => self.warning,
            Status::Info => self.info,
            Status::Starting => self.starting,
            Status::Inactive => self.inactive,
        }
    }

    pub fn process_dot(&self, status: Status, is_dimmed: bool) -> (Style, Style) {
        let dimmed = |style: Style| {
            if is_dimmed {
                style.layered(self.dimmed)
            } else {
                style
            }
        };
        (dimmed(self.status(status)), dimmed(self.inactive))
    }

    pub fn part(&self, part: Part, highlight: Highlight) -> Style {
        let style = match part {
            Part::Message => self.message,
            Part::Name => self.name,
            Part::NestedName => self.nested_name,
            Part::Detail => self.detail,
            Part::Elapsed => self.elapsed,
        };
        self.highlighted(style, part, highlight)
    }

    pub fn highlighted(&self, style: Style, part: Part, highlight: Highlight) -> Style {
        match highlight {
            Highlight::None => style,
            Highlight::Selected => {
                let selected = match part {
                    Part::Message | Part::Name | Part::NestedName => self.selected_name,
                    Part::Detail => self.selected_detail,
                    Part::Elapsed => self.selected_elapsed,
                };
                style.layered(Style {
                    reverse: selected.reverse && !self.reverses_row(highlight),
                    ..selected
                })
            }
            Highlight::Dimmed => style.layered(self.dimmed),
        }
    }

    pub fn row_background(&self, highlight: Highlight) -> Option<Color> {
        match highlight {
            Highlight::Selected => self.selection.background,
            Highlight::None | Highlight::Dimmed => None,
        }
    }

    pub fn reverses_row(&self, highlight: Highlight) -> bool {
        highlight == Highlight::Selected && self.selection.or_reversed().reverse
    }
}

impl Default for ActivityTheme {
    fn default() -> Self {
        Self::new(&ThemeConfig::default())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogTheme {
    pub text: Style,
    pub title: Style,
    pub line_number: Style,
    pub filler: Style,
    pub selection: Style,
    pub search_match: Style,
    pub current_match: Style,
}

impl LogTheme {
    pub fn new(theme: &ThemeConfig) -> Self {
        let style = |scopes: &[&str]| theme.style(&[&["logs"], scopes].concat());
        let text = style(&[]);
        let marked = |mark: Style, style: Style| Style {
            reverse: text.reverse != mark.or_reversed().reverse,
            ..style
        };
        let selection = if theme.styles.contains_key("logs.selection") {
            theme.user_style(&["logs.selection"])
        } else {
            theme.style(&["logs.selection"])
        };
        Self {
            text,
            title: style(&["logs.title"]),
            line_number: style(&["logs.line_number"]),
            filler: style(&["logs.filler"]),
            selection: marked(selection, text.layered(selection)),
            search_match: style(&["logs.search", "logs.search.match"]),
            current_match: marked(
                theme.style(&["logs.search", "logs.search.current"]),
                style(&["logs.search", "logs.search.current"]),
            ),
        }
    }
}

impl Default for LogTheme {
    fn default() -> Self {
        Self::new(&ThemeConfig::default())
    }
}

fn preset_style(preset: ThemePreset, scope: &str) -> Style {
    match preset {
        ThemePreset::Devenv => devenv_style(scope),
        ThemePreset::Terminal => terminal_style(scope),
        ThemePreset::None => none_style(scope),
    }
}

fn devenv_style(scope: &str) -> Style {
    match scope {
        "statusline" => Style::foreground(Color::Reset),
        "statusline.separator" => Style::foreground(COLOR_HIERARCHY),
        "statusline.key_hints" | "statusline.pending_key" => Style::foreground(COLOR_INTERACTIVE),
        "statusline.summary"
        | "statusline.builds"
        | "statusline.downloads"
        | "statusline.queries"
        | "statusline.tasks"
        | "statusline.processes" => Style::foreground(COLOR_COMPLETED),
        "activity" => Style::foreground(Color::Reset),
        "activity.name" | "activity.progress" | "activity.status.active" => {
            Style::foreground(COLOR_ACTIVE)
        }
        "activity.name.nested" => Style::foreground(COLOR_ACTIVE_NESTED),
        "activity.detail" => Style::foreground(COLOR_SECONDARY),
        "activity.elapsed" | "activity.tree" | "activity.status.inactive" | "activity.dimmed" => {
            Style::foreground(COLOR_HIERARCHY)
        }
        "activity.log" => Style::foreground(Color::AnsiValue(245)),
        "activity.progress.track" => Style::foreground(Color::AnsiValue(238)),
        "activity.status.success" => Style::foreground(COLOR_COMPLETED),
        "activity.status.failure" => Style::foreground(COLOR_FAILED),
        "activity.status.warning" | "activity.status.starting" => {
            Style::foreground(COLOR_TRANSIENT)
        }
        "activity.status.info" => Style::foreground(COLOR_INFO),
        "activity.selection" | "logs.selection" => {
            Style::colors(Color::AnsiValue(232), Color::AnsiValue(250))
        }
        "activity.selection.detail" | "activity.selection.elapsed" => {
            Style::foreground(Color::AnsiValue(238))
        }
        "logs" => Style::foreground(Color::AnsiValue(250)),
        "logs.title" => Style {
            bold: true,
            ..Style::foreground(Color::Cyan)
        },
        "logs.filler" => Style::foreground(Color::AnsiValue(238)),
        "logs.search.match" => Style::colors(Color::AnsiValue(230), Color::AnsiValue(58)),
        "logs.search.current" => Style::colors(Color::AnsiValue(232), Color::AnsiValue(220)),
        _ => Style::default(),
    }
}

fn terminal_style(scope: &str) -> Style {
    match scope {
        "statusline" => Style::foreground(Color::Reset),
        "statusline.separator" => Style::foreground(Color::DarkGrey),
        "statusline.key_hints" | "statusline.pending_key" => Style::foreground(Color::Yellow),
        "statusline.summary"
        | "statusline.builds"
        | "statusline.downloads"
        | "statusline.queries"
        | "statusline.tasks"
        | "statusline.processes" => Style::foreground(Color::Green),
        "activity" => Style::foreground(Color::Reset),
        "activity.name.nested"
        | "activity.detail"
        | "activity.elapsed"
        | "activity.tree"
        | "activity.progress.track"
        | "activity.status.inactive"
        | "activity.dimmed" => Style::foreground(Color::DarkGrey),
        "activity.status.success" => Style::foreground(Color::Green),
        "activity.status.failure" => Style::foreground(Color::Red),
        "activity.status.warning" | "activity.status.starting" => Style::foreground(Color::Yellow),
        "activity.status.info" => Style::foreground(Color::Blue),
        "activity.selection" | "logs.selection" => Style::colors(Color::Black, Color::White),
        "logs" => Style::foreground(Color::Reset),
        "logs.title" => Style {
            bold: true,
            ..Style::foreground(Color::Cyan)
        },
        "logs.line_number" | "logs.filler" => Style::foreground(Color::DarkGrey),
        "logs.search.match" => Style::colors(Color::Black, Color::DarkYellow),
        "logs.search.current" => Style::colors(Color::Black, Color::Yellow),
        _ => Style::default(),
    }
}

fn none_style(scope: &str) -> Style {
    match scope {
        "activity.progress.track"
        | "activity.status.inactive"
        | "activity.dimmed"
        | "logs.line_number"
        | "logs.filler" => Style {
            dim: true,
            ..Style::default()
        },
        "logs.title" => Style {
            bold: true,
            ..Style::default()
        },
        "logs.search.match" => Style {
            underline: true,
            ..Style::default()
        },
        _ => Style::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ACTIVITY_SCOPES, ColorSpec, LOG_SCOPES};

    fn theme(preset: ThemePreset, styles: &[(&str, StyleConfig)]) -> ThemeConfig {
        ThemeConfig {
            preset,
            styles: styles
                .iter()
                .map(|(scope, style)| (scope.to_string(), style.clone()))
                .collect(),
            ..ThemeConfig::default()
        }
    }

    fn foreground_config(color: &str) -> StyleConfig {
        StyleConfig {
            foreground: Some(ColorSpec(color.to_string())),
            ..StyleConfig::default()
        }
    }

    #[test]
    fn devenv_preset_keeps_the_built_in_activity_colors() {
        let theme = ActivityTheme::default();

        assert_eq!(theme.name.foreground, Some(COLOR_ACTIVE));
        assert_eq!(theme.nested_name.foreground, Some(COLOR_ACTIVE_NESTED));
        assert_eq!(theme.detail.foreground, Some(COLOR_SECONDARY));
        assert_eq!(theme.tree.foreground, Some(COLOR_HIERARCHY));
        assert_eq!(theme.log.foreground, Some(Color::AnsiValue(245)));
        assert_eq!(
            theme.status(Status::Success).foreground,
            Some(COLOR_COMPLETED)
        );
        assert_eq!(theme.status(Status::Failure).foreground, Some(COLOR_FAILED));
        assert_eq!(
            theme.part(Part::NestedName, Highlight::Selected),
            Style {
                foreground: Some(Color::AnsiValue(232)),
                background: Some(Color::AnsiValue(250)),
                ..Style::default()
            }
        );
        assert_eq!(
            theme.row_background(Highlight::Selected),
            Some(Color::AnsiValue(250))
        );
    }

    #[test]
    fn user_styles_override_presets_from_broad_to_narrow_scopes() {
        let theme = ActivityTheme::new(&theme(
            ThemePreset::Devenv,
            &[
                ("activity.name", foreground_config("blue")),
                (
                    "activity.status",
                    StyleConfig {
                        modifiers: vec![TextModifier::Bold],
                        ..StyleConfig::default()
                    },
                ),
                ("activity.status.failure", foreground_config("magenta")),
            ],
        ));

        assert_eq!(theme.name.foreground, Some(Color::Blue));
        assert_eq!(theme.nested_name.foreground, Some(Color::Blue));
        assert_eq!(theme.detail.foreground, Some(COLOR_SECONDARY));
        assert_eq!(
            theme.status(Status::Failure),
            Style {
                foreground: Some(Color::Magenta),
                bold: true,
                ..Style::default()
            }
        );
        assert_eq!(
            theme.status(Status::Success),
            Style {
                foreground: Some(COLOR_COMPLETED),
                bold: true,
                ..Style::default()
            }
        );
    }

    #[test]
    fn configured_selection_replaces_preset_selection() {
        let theme = ActivityTheme::new(&theme(
            ThemePreset::Devenv,
            &[(
                "activity.selection",
                StyleConfig {
                    background: Some(ColorSpec("#373e47".to_string())),
                    ..StyleConfig::default()
                },
            )],
        ));
        let selected = |part| theme.part(part, Highlight::Selected).foreground;

        assert_eq!(selected(Part::Name), Some(COLOR_ACTIVE));
        assert_eq!(selected(Part::Detail), Some(COLOR_SECONDARY));
        assert_eq!(selected(Part::Elapsed), Some(COLOR_HIERARCHY));
        assert!(!theme.reverses_row(Highlight::Selected));
    }

    #[test]
    fn selection_layers_over_part_styles() {
        let theme = ActivityTheme::new(&theme(
            ThemePreset::Devenv,
            &[
                ("activity.name", foreground_config("cyan")),
                (
                    "activity.selection",
                    StyleConfig {
                        foreground: Some(ColorSpec("default".to_string())),
                        background: Some(ColorSpec("#373e47".to_string())),
                        ..StyleConfig::default()
                    },
                ),
            ],
        ));
        let selected = |part| theme.part(part, Highlight::Selected);
        let background = Some(Color::Rgb {
            r: 0x37,
            g: 0x3e,
            b: 0x47,
        });

        assert_eq!(theme.name.foreground, Some(Color::Cyan));
        assert_eq!(selected(Part::Name).foreground, Some(Color::Reset));
        assert_eq!(
            selected(Part::Detail),
            Style {
                foreground: Some(Color::Reset),
                background,
                ..Style::default()
            }
        );
        assert_eq!(theme.row_background(Highlight::Selected), background);
        assert_eq!(theme.row_background(Highlight::Dimmed), None);
    }

    #[test]
    fn terminal_preset_uses_terminal_colors() {
        let theme = ActivityTheme::new(&theme(ThemePreset::Terminal, &[]));

        assert_eq!(theme.name.foreground, Some(Color::Reset));
        assert_eq!(theme.tree.foreground, Some(Color::DarkGrey));
        assert_eq!(theme.status(Status::Success).foreground, Some(Color::Green));
        assert_eq!(theme.status(Status::Failure).foreground, Some(Color::Red));
        assert_eq!(
            theme.status(Status::Warning).foreground,
            Some(Color::Yellow)
        );
        assert_eq!(theme.status(Status::Info).foreground, Some(Color::Blue));
        assert_eq!(
            theme.selection,
            Style {
                foreground: Some(Color::Black),
                background: Some(Color::White),
                ..Style::default()
            }
        );
    }

    #[test]
    fn none_preset_marks_selection_and_dimming_without_colors() {
        let theme = ActivityTheme::new(&theme(ThemePreset::None, &[]));

        assert_eq!(theme.name, Style::default());
        assert_eq!(theme.status(Status::Failure), Style::default());
        assert_eq!(
            theme.part(Part::Name, Highlight::Selected),
            Style::default()
        );
        assert!(theme.reverses_row(Highlight::Selected));
        assert!(!theme.reverses_row(Highlight::Dimmed));
        assert_eq!(
            theme.part(Part::Detail, Highlight::Dimmed),
            Style {
                dim: true,
                ..Style::default()
            }
        );
        assert_eq!(theme.row_background(Highlight::Selected), None);
        assert!(theme.status(Status::Inactive).dim);
        assert!(theme.progress_track.dim);
    }

    #[test]
    fn colored_selection_replaces_reverse_video() {
        let theme = ActivityTheme::new(&theme(
            ThemePreset::None,
            &[(
                "activity.selection",
                StyleConfig {
                    background: Some(ColorSpec("blue".to_string())),
                    ..StyleConfig::default()
                },
            )],
        ));

        assert!(!theme.reverses_row(Highlight::Selected));
        assert_eq!(theme.selected_name.background, Some(Color::Blue));
        assert_eq!(theme.row_background(Highlight::Selected), Some(Color::Blue));
    }

    #[test]
    fn dimmed_process_dots_dim_both_pulse_beats() {
        let theme = ActivityTheme::new(&theme(ThemePreset::None, &[]));

        assert_eq!(
            theme.process_dot(Status::Starting, false),
            (Style::default(), theme.status(Status::Inactive))
        );
        let (style, pulse_style) = theme.process_dot(Status::Starting, true);
        assert!(style.dim);
        assert!(pulse_style.dim);

        let theme = ActivityTheme::default();
        assert_eq!(
            theme.process_dot(Status::Starting, true),
            (
                Style::foreground(COLOR_HIERARCHY),
                Style::foreground(COLOR_HIERARCHY)
            )
        );
    }

    #[test]
    fn every_activity_scope_is_resolved() {
        for preset in [
            ThemePreset::Devenv,
            ThemePreset::Terminal,
            ThemePreset::None,
        ] {
            for scope in ACTIVITY_SCOPES {
                let marked = ActivityTheme::new(&theme(
                    preset,
                    &[(
                        scope,
                        StyleConfig {
                            modifiers: vec![TextModifier::Underline],
                            ..StyleConfig::default()
                        },
                    )],
                ));
                assert_ne!(
                    marked,
                    ActivityTheme::new(&theme(preset, &[])),
                    "{preset:?} {scope}"
                );
            }
        }
    }

    #[test]
    fn every_log_scope_is_resolved() {
        for preset in [
            ThemePreset::Devenv,
            ThemePreset::Terminal,
            ThemePreset::None,
        ] {
            for scope in LOG_SCOPES {
                let marked = LogTheme::new(&theme(
                    preset,
                    &[(
                        scope,
                        StyleConfig {
                            modifiers: vec![TextModifier::Italic],
                            ..StyleConfig::default()
                        },
                    )],
                ));
                assert_ne!(
                    marked,
                    LogTheme::new(&theme(preset, &[])),
                    "{preset:?} {scope}"
                );
            }
        }
    }

    #[test]
    fn devenv_preset_keeps_the_built_in_log_colors() {
        let theme = LogTheme::default();
        let text = Style::foreground(Color::AnsiValue(250));

        assert_eq!(theme.text, text);
        assert_eq!(theme.line_number, text);
        assert_eq!(
            theme.title,
            Style {
                bold: true,
                ..Style::foreground(Color::Cyan)
            }
        );
        assert_eq!(theme.filler, Style::foreground(Color::AnsiValue(238)));
        assert_eq!(
            theme.selection,
            Style::colors(Color::AnsiValue(232), Color::AnsiValue(250))
        );
        assert_eq!(
            theme.search_match,
            Style::colors(Color::AnsiValue(230), Color::AnsiValue(58))
        );
        assert_eq!(
            theme.current_match,
            Style::colors(Color::AnsiValue(232), Color::AnsiValue(220))
        );
    }

    #[test]
    fn configured_log_selection_replaces_preset_selection() {
        let theme = LogTheme::new(&theme(
            ThemePreset::Devenv,
            &[(
                "logs.selection",
                StyleConfig {
                    background: Some(ColorSpec("#373e47".to_string())),
                    ..StyleConfig::default()
                },
            )],
        ));

        assert_eq!(
            theme.selection,
            Style {
                background: Some(Color::Rgb {
                    r: 0x37,
                    g: 0x3e,
                    b: 0x47,
                }),
                ..theme.text
            }
        );
    }

    #[test]
    fn log_highlights_flip_reversed_text() {
        let theme = LogTheme::new(&theme(
            ThemePreset::None,
            &[(
                "logs",
                StyleConfig {
                    modifiers: vec![TextModifier::Reverse],
                    ..StyleConfig::default()
                },
            )],
        ));

        assert!(theme.text.reverse);
        assert!(!theme.selection.reverse);
        assert!(!theme.current_match.reverse);
    }

    #[test]
    fn none_preset_marks_log_highlights_without_colors() {
        let plain = LogTheme::new(&theme(ThemePreset::None, &[]));

        assert_eq!(plain.text, Style::default());
        assert!(plain.line_number.dim);
        assert!(plain.selection.reverse);
        assert!(plain.current_match.reverse);
        assert!(plain.search_match.underline);
        assert!(!plain.search_match.reverse);

        let colored = LogTheme::new(&theme(
            ThemePreset::None,
            &[("logs.selection", foreground_config("blue"))],
        ));
        assert!(!colored.selection.reverse);
    }
}
