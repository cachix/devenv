use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::style::Color;
use devenv_shell::keybindings::{ShellAction, ShellKeyChord, ShellKeyCode, ShellKeybindings};
use miette::{Diagnostic, NamedSource, SourceSpan};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::Instant;
use thiserror::Error;

pub const USER_CONFIG_VERSION: u32 = 1;
const USER_CONFIG_SCHEMA_URL: &str = "https://devenv.sh/devenv.user.schema.json";
const MAX_USER_CONFIG_BYTES: u64 = 1024 * 1024;
const KEY_CHORD_PATTERN: &str = r"(?:(?:ctrl|alt|shift)\+)*(?:enter|esc|escape|backspace|delete|insert|up|down|left|right|home|end|page[_-]up|page[_-]down|tab|back[_-]tab|space|f(?:[1-9]|1[0-9]|2[0-4])|[^\s+A-Z])";
const RESERVED_CTRL_C_PATTERN: &str =
    r"(?:^|[\t\n\r ])(?:(?:ctrl|shift)\+)*ctrl\+(?:(?:ctrl|shift)\+)*c(?:$|[\t\n\r ])";

#[derive(Debug, Error, Diagnostic)]
pub enum UserConfigError {
    #[error("failed to read user configuration at {path}")]
    #[diagnostic(code(devenv::user_config::read))]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid YAML in user configuration")]
    #[diagnostic(code(devenv::user_config::yaml))]
    Parse {
        #[source_code]
        source_code: NamedSource<String>,
        #[label("{message}")]
        span: Option<SourceSpan>,
        message: String,
    },
    #[error("invalid user configuration: {message}")]
    #[diagnostic(code(devenv::user_config::validation))]
    Validation { message: String },
    #[error("failed to serialize resolved user configuration")]
    #[diagnostic(code(devenv::user_config::serialize))]
    Serialize {
        #[source]
        source: serde_yaml::Error,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UserConfig {
    #[schemars(range(min = 1, max = 1))]
    pub version: u32,
    #[serde(default)]
    pub tui: TuiPreferences,
    #[serde(default)]
    pub shell: ShellPreferences,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            version: USER_CONFIG_VERSION,
            tui: TuiPreferences::default(),
            shell: ShellPreferences::default(),
        }
    }
}

impl UserConfig {
    pub fn from_yaml(path: impl AsRef<Path>, input: String) -> Result<Self, UserConfigError> {
        let path = path.as_ref();
        if let Err(error) = serde_yaml::from_str::<serde_yaml::Value>(&input) {
            return Err(parse_error(path, input, error));
        }
        let parsed = serde_yaml::from_str::<Self>(&input)
            .map_err(|error| parse_error(path, input, error))?;
        parsed.validate()?;
        Ok(parsed)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, UserConfigError> {
        let path = path.as_ref();
        let file = std::fs::File::open(path).map_err(|source| UserConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let mut input = String::new();
        file.take(MAX_USER_CONFIG_BYTES + 1)
            .read_to_string(&mut input)
            .map_err(|source| UserConfigError::Read {
                path: path.to_path_buf(),
                source,
            })?;
        if input.len() as u64 > MAX_USER_CONFIG_BYTES {
            return Err(UserConfigError::Read {
                path: path.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "user configuration exceeds the 1 MiB size limit",
                ),
            });
        }
        Self::from_yaml(path, input)
    }

    pub fn to_yaml(&self) -> Result<String, UserConfigError> {
        serde_yaml::to_string(self)
            .map(|config| {
                format!("# yaml-language-server: $schema={USER_CONFIG_SCHEMA_URL}\n\n{config}")
            })
            .map_err(|source| UserConfigError::Serialize { source })
    }

    pub fn validate(&self) -> Result<(), UserConfigError> {
        if self.version != USER_CONFIG_VERSION {
            return validation(format!(
                "unsupported version {}; expected {USER_CONFIG_VERSION}",
                self.version
            ));
        }
        self.tui.validate()?;
        self.shell.resolve()?;
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ShellPreferences {
    #[schemars(schema_with = "shell_keybindings_schema")]
    pub keybindings: BTreeMap<String, Vec<String>>,
}

impl ShellPreferences {
    pub fn resolve(&self) -> Result<ShellKeybindings, UserConfigError> {
        let mut keybindings = ShellKeybindings::default();
        for (name, configured) in &self.keybindings {
            let Some(action) = ShellAction::parse(name) else {
                return validation(format!("unknown action shell.keybindings.{name}"));
            };
            let mut resolved = Vec::with_capacity(configured.len());
            for binding in configured {
                let sequence =
                    KeySequence::parse(binding).map_err(|message| UserConfigError::Validation {
                        message: format!("shell.keybindings.{name}: {message}"),
                    })?;
                if sequence.chords.len() != 1 {
                    return validation(format!(
                        "shell.keybindings.{name}: expected exactly one key chord"
                    ));
                }
                resolved.push(sequence.chords[0].to_shell_chord());
            }
            keybindings.replace(action, resolved);
        }
        if let Some(conflict) = keybindings.conflict() {
            return validation(format!(
                "shell.keybindings assigns {:?} to both {:?} and {:?}",
                conflict.chord,
                conflict.first.as_str(),
                conflict.second.as_str()
            ));
        }
        Ok(keybindings)
    }
}

fn parse_error(path: &Path, input: String, error: serde_yaml::Error) -> UserConfigError {
    let span = error.location().map(|location| {
        let offset = location.index().min(input.len().saturating_sub(1));
        (offset, usize::from(!input.is_empty())).into()
    });
    UserConfigError::Parse {
        source_code: NamedSource::new(path.display().to_string(), input),
        span,
        message: error.to_string(),
    }
}

#[derive(Clone, Debug, Default)]
pub struct TuiRunContext {
    pub profiles: Vec<String>,
    pub project_root: Option<PathBuf>,
    pub command: Option<String>,
    pub shell: Option<String>,
    pub started_at: Option<Instant>,
}

impl TuiRunContext {
    pub fn project_name(&self) -> Option<String> {
        self.project_root
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TuiPreferences {
    pub viewport: ViewportPlacement,
    pub theme: ThemeConfig,
    pub statusline: StatuslineConfig,
    pub keybindings: KeybindingsConfig,
    pub behavior: BehaviorConfig,
}

impl TuiPreferences {
    pub fn validate(&self) -> Result<(), UserConfigError> {
        self.theme.validate()?;
        self.statusline.validate(&self.theme.palette)?;
        for scope in self.theme.styles.keys() {
            if let Some(component) = scope.strip_prefix("statusline.")
                && component != "separator"
                && !builtin_component(component)
                && !self.statusline.components.contains_key(component)
            {
                return validation(format!(
                    "tui.theme.styles.{scope} references unknown statusline component {component:?}"
                ));
            }
        }
        self.keybindings.resolve()?;
        self.behavior.validate()
    }

    pub fn uses_default_statusline(&self) -> bool {
        self.statusline.uses_default_layouts()
            && self.theme.preset == ThemePreset::Devenv
            && self.theme.palette.is_empty()
            && self.theme.styles.is_empty()
            && self.keybindings.is_default()
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ViewportPlacement {
    #[default]
    Inline,
    Top,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreset {
    #[default]
    Devenv,
    Terminal,
    None,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    pub preset: ThemePreset,
    pub palette: BTreeMap<String, ColorSpec>,
    pub styles: BTreeMap<String, StyleConfig>,
}

impl ThemeConfig {
    fn validate(&self) -> Result<(), UserConfigError> {
        for (name, color) in &self.palette {
            if !valid_name(name, false) {
                return validation(format!("tui.theme.palette.{name} is not a valid name"));
            }
            color
                .parse_direct()
                .map_err(|message| UserConfigError::Validation {
                    message: format!("tui.theme.palette.{name}: {message}"),
                })?;
        }
        for (scope, style) in &self.styles {
            let supported = scope == "statusline"
                || scope == "statusline.separator"
                || scope
                    .strip_prefix("statusline.")
                    .is_some_and(|name| valid_name(name, false));
            if !supported {
                return validation(format!(
                    "tui.theme.styles.{scope} is not a supported style scope"
                ));
            }
            style.validate(&self.palette, &format!("tui.theme.styles.{scope}"))?;
        }
        Ok(())
    }

    pub fn resolve_color(&self, value: &ColorSpec) -> Result<Color, String> {
        if let Some(color) = self.palette.get(&value.0) {
            color.parse_direct()
        } else {
            value.parse_direct()
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(transparent)]
pub struct ColorSpec(
    #[schemars(regex(
        pattern = r"^(?:#[0-9A-Fa-f]{6}|ansi:0*(?:[0-9]|[1-9][0-9]|1[0-9]{2}|2[0-4][0-9]|25[0-5])|[A-Za-z0-9_-]+)$"
    ))]
    pub String,
);

impl ColorSpec {
    pub fn parse_direct(&self) -> Result<Color, String> {
        let value = self.0.as_str();
        if let Some(raw) = value.strip_prefix("ansi:") {
            return raw
                .parse::<u8>()
                .map(Color::AnsiValue)
                .map_err(|_| format!("{value:?} must use ansi:0 through ansi:255"));
        }
        if let Some(raw) = value.strip_prefix('#') {
            if raw.len() != 6 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(format!("{value:?} must use #RRGGBB"));
            }
            let channel = |range| u8::from_str_radix(&raw[range], 16).unwrap();
            return Ok(Color::Rgb {
                r: channel(0..2),
                g: channel(2..4),
                b: channel(4..6),
            });
        }
        let color = match value {
            "default" => Color::Reset,
            "black" => Color::Black,
            "dark_grey" => Color::DarkGrey,
            "red" => Color::Red,
            "dark_red" => Color::DarkRed,
            "green" => Color::Green,
            "dark_green" => Color::DarkGreen,
            "yellow" => Color::Yellow,
            "dark_yellow" => Color::DarkYellow,
            "blue" => Color::Blue,
            "dark_blue" => Color::DarkBlue,
            "magenta" => Color::Magenta,
            "dark_magenta" => Color::DarkMagenta,
            "cyan" => Color::Cyan,
            "dark_cyan" => Color::DarkCyan,
            "white" => Color::White,
            "grey" => Color::Grey,
            _ => {
                return Err(format!(
                    "unknown color {value:?}; use a palette name, terminal color, ansi:N, or #RRGGBB"
                ));
            }
        };
        Ok(color)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextModifier {
    Bold,
    Dim,
    Italic,
    Underline,
    Reverse,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct StyleConfig {
    pub foreground: Option<ColorSpec>,
    pub background: Option<ColorSpec>,
    #[schemars(extend("uniqueItems" = true))]
    pub modifiers: Vec<TextModifier>,
}

impl StyleConfig {
    fn validate(
        &self,
        palette: &BTreeMap<String, ColorSpec>,
        path: &str,
    ) -> Result<(), UserConfigError> {
        for (field, value) in [
            ("foreground", self.foreground.as_ref()),
            ("background", self.background.as_ref()),
        ] {
            if let Some(value) = value
                && !palette.contains_key(&value.0)
            {
                value
                    .parse_direct()
                    .map_err(|message| UserConfigError::Validation {
                        message: format!("{path}.{field}: {message}"),
                    })?;
            }
        }
        let mut modifiers = BTreeSet::new();
        for modifier in &self.modifiers {
            if !modifiers.insert(format!("{modifier:?}")) {
                return validation(format!(
                    "{path}.modifiers contains {modifier:?} more than once"
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct StatuslineConfig {
    pub enabled: bool,
    pub position: StatuslinePosition,
    #[schemars(length(max = 8))]
    pub separator: String,
    pub layouts: StatuslineLayouts,
    pub components: BTreeMap<String, StatusComponentConfig>,
}

impl Default for StatuslineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            position: StatuslinePosition::Inline,
            separator: " │ ".to_string(),
            layouts: StatuslineLayouts::default(),
            components: BTreeMap::new(),
        }
    }
}

impl StatuslineConfig {
    pub fn uses_default_main_layout(&self) -> bool {
        self.enabled
            && self.separator == " │ "
            && self.components.is_empty()
            && self.layouts.main.left == ["summary"]
            && self.layouts.main.center.is_empty()
            && self.layouts.main.right == ["key_hints"]
    }

    pub fn uses_default_layouts(&self) -> bool {
        let defaults = Self::default();
        self.enabled
            && self.separator == defaults.separator
            && self.components.is_empty()
            && layout_matches(&self.layouts.main, &defaults.layouts.main)
            && layout_matches(&self.layouts.logs, &defaults.layouts.logs)
            && layout_matches(&self.layouts.search, &defaults.layouts.search)
            && layout_matches(&self.layouts.prompt, &defaults.layouts.prompt)
    }

    fn validate(&self, palette: &BTreeMap<String, ColorSpec>) -> Result<(), UserConfigError> {
        if self.separator.chars().any(char::is_control) || display_width(&self.separator) > 8 {
            return validation(
                "tui.statusline.separator must be a single-line value at most 8 columns wide",
            );
        }
        for (name, component) in &self.components {
            if !valid_name(name, false) {
                return validation(format!(
                    "tui.statusline.components.{name} is not a valid name"
                ));
            }
            component.validate(name, palette)?;
        }
        for (name, layout) in self.layouts.iter() {
            let mut seen = BTreeSet::new();
            for component in layout.all() {
                if !seen.insert(component) {
                    return validation(format!(
                        "tui.statusline.layouts.{name} contains component {component:?} more than once"
                    ));
                }
                if !builtin_component(component) && !self.components.contains_key(component) {
                    return validation(format!(
                        "tui.statusline.layouts.{name} references unknown component {component:?}"
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatuslinePosition {
    Top,
    Bottom,
    #[default]
    Inline,
}

fn layout_matches(left: &StatuslineLayout, right: &StatuslineLayout) -> bool {
    left.left == right.left && left.center == right.center && left.right == right.right
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct StatuslineLayouts {
    pub main: StatuslineLayout,
    pub logs: StatuslineLayout,
    pub search: StatuslineLayout,
    pub prompt: StatuslineLayout,
}

impl Default for StatuslineLayouts {
    fn default() -> Self {
        Self {
            main: StatuslineLayout {
                left: vec!["summary".to_string()],
                center: vec![],
                right: vec!["key_hints".to_string()],
            },
            logs: StatuslineLayout {
                left: vec!["log_mode".to_string(), "log_position".to_string()],
                center: vec![],
                right: vec!["key_hints".to_string()],
            },
            search: StatuslineLayout {
                left: vec!["search".to_string()],
                center: vec![],
                right: vec!["key_hints".to_string()],
            },
            prompt: StatuslineLayout {
                left: vec!["prompt".to_string()],
                center: vec![],
                right: vec!["key_hints".to_string()],
            },
        }
    }
}

impl StatuslineLayouts {
    fn iter(&self) -> [(&'static str, &StatuslineLayout); 4] {
        [
            ("main", &self.main),
            ("logs", &self.logs),
            ("search", &self.search),
            ("prompt", &self.prompt),
        ]
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct StatuslineLayout {
    #[schemars(extend("uniqueItems" = true))]
    pub left: Vec<String>,
    #[schemars(extend("uniqueItems" = true))]
    pub center: Vec<String>,
    #[schemars(extend("uniqueItems" = true))]
    pub right: Vec<String>,
}

impl StatuslineLayout {
    pub fn all(&self) -> impl Iterator<Item = &String> {
        self.left.iter().chain(&self.center).chain(&self.right)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatusComponentKind {
    Summary,
    Builds,
    Downloads,
    Queries,
    Tasks,
    Processes,
    Profiles,
    Project,
    Command,
    Shell,
    Elapsed,
    Selected,
    LogMode,
    LogPosition,
    RetainedLogs,
    Search,
    Prompt,
    PendingKey,
    KeyHints,
    Text,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatusOverflow {
    Truncate,
    #[default]
    Hide,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct StatusComponentConfig {
    #[serde(rename = "type")]
    pub kind: Option<StatusComponentKind>,
    #[schemars(length(max = 512))]
    pub format: Option<String>,
    #[schemars(length(max = 512))]
    pub compact_format: Option<String>,
    pub priority: u8,
    pub required: bool,
    #[schemars(range(min = 1))]
    pub max_width: Option<u16>,
    pub overflow: StatusOverflow,
    pub show_empty: bool,
    #[schemars(length(max = 256))]
    pub text: Option<String>,
    pub style: StyleConfig,
}

impl Default for StatusComponentConfig {
    fn default() -> Self {
        Self {
            kind: None,
            format: None,
            compact_format: None,
            priority: 50,
            required: false,
            max_width: None,
            overflow: StatusOverflow::Truncate,
            show_empty: false,
            text: None,
            style: StyleConfig::default(),
        }
    }
}

impl StatusComponentConfig {
    fn validate(
        &self,
        name: &str,
        palette: &BTreeMap<String, ColorSpec>,
    ) -> Result<(), UserConfigError> {
        let kind = self.kind.or_else(|| builtin_kind(name));
        let Some(kind) = kind else {
            return validation(format!(
                "tui.statusline.components.{name}.type is required for custom components"
            ));
        };
        if kind == StatusComponentKind::Text {
            let Some(text) = self.text.as_deref() else {
                return validation(format!(
                    "tui.statusline.components.{name}.text is required for text components"
                ));
            };
            validate_single_line(text, 256, &format!("tui.statusline.components.{name}.text"))?;
        } else if self.text.is_some() {
            return validation(format!(
                "tui.statusline.components.{name}.text is only valid for text components"
            ));
        }
        if let Some(width) = self.max_width
            && width == 0
        {
            return validation(format!(
                "tui.statusline.components.{name}.max_width must be greater than zero"
            ));
        }
        for (field, format) in [
            ("format", self.format.as_deref()),
            ("compact_format", self.compact_format.as_deref()),
        ] {
            if let Some(format) = format {
                validate_single_line(
                    format,
                    512,
                    &format!("tui.statusline.components.{name}.{field}"),
                )?;
                validate_format(kind, format).map_err(|message| UserConfigError::Validation {
                    message: format!("tui.statusline.components.{name}.{field}: {message}"),
                })?;
            }
        }
        self.style
            .validate(palette, &format!("tui.statusline.components.{name}.style"))
    }
}

fn builtin_component(name: &str) -> bool {
    builtin_kind(name).is_some()
}

pub fn builtin_kind(name: &str) -> Option<StatusComponentKind> {
    Some(match name {
        "summary" => StatusComponentKind::Summary,
        "builds" => StatusComponentKind::Builds,
        "downloads" => StatusComponentKind::Downloads,
        "queries" => StatusComponentKind::Queries,
        "tasks" => StatusComponentKind::Tasks,
        "processes" => StatusComponentKind::Processes,
        "profiles" => StatusComponentKind::Profiles,
        "project" => StatusComponentKind::Project,
        "command" => StatusComponentKind::Command,
        "shell" => StatusComponentKind::Shell,
        "elapsed" => StatusComponentKind::Elapsed,
        "selected" => StatusComponentKind::Selected,
        "log_mode" => StatusComponentKind::LogMode,
        "log_position" => StatusComponentKind::LogPosition,
        "retained_logs" => StatusComponentKind::RetainedLogs,
        "search" => StatusComponentKind::Search,
        "prompt" => StatusComponentKind::Prompt,
        "pending_key" => StatusComponentKind::PendingKey,
        "key_hints" => StatusComponentKind::KeyHints,
        _ => return None,
    })
}

fn validate_format(kind: StatusComponentKind, format: &str) -> Result<(), String> {
    let allowed = format_variables(kind);
    let chars = format.as_bytes();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == b'{' {
            if chars.get(index + 1) == Some(&b'{') {
                index += 2;
                continue;
            }
            let Some(end) = format[index + 1..].find('}') else {
                return Err("unclosed '{'; use '{{' for a literal brace".to_string());
            };
            let variable = &format[index + 1..index + 1 + end];
            if !allowed.contains(&variable) {
                return Err(format!(
                    "unknown variable {{{variable}}}; valid variables are {}",
                    allowed
                        .iter()
                        .map(|name| format!("{{{name}}}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            index += end + 2;
        } else if chars[index] == b'}' {
            if chars.get(index + 1) == Some(&b'}') {
                index += 2;
            } else {
                return Err("unmatched '}'; use '}}' for a literal brace".to_string());
            }
        } else {
            index += 1;
        }
    }
    Ok(())
}

fn format_variables(kind: StatusComponentKind) -> &'static [&'static str] {
    match kind {
        StatusComponentKind::Summary => &["summary"],
        StatusComponentKind::Builds
        | StatusComponentKind::Downloads
        | StatusComponentKind::Queries
        | StatusComponentKind::Tasks => &["active", "completed", "failed", "total", "expected"],
        StatusComponentKind::Processes => &["running", "stopped", "failed", "hidden", "total"],
        StatusComponentKind::Profiles => &["profiles", "count"],
        StatusComponentKind::Project => &["name", "path"],
        StatusComponentKind::Command => &["command"],
        StatusComponentKind::Shell => &["shell"],
        StatusComponentKind::Elapsed => &["elapsed"],
        StatusComponentKind::Selected => &["name", "status"],
        StatusComponentKind::LogMode => &["mode"],
        StatusComponentKind::LogPosition => &["current", "total", "percent"],
        StatusComponentKind::RetainedLogs => &["retained", "discarded", "total"],
        StatusComponentKind::Search => &["query", "current", "total", "result"],
        StatusComponentKind::Prompt => &["prompt"],
        StatusComponentKind::PendingKey => &["keys"],
        StatusComponentKind::KeyHints => &["hints"],
        StatusComponentKind::Text => &["text"],
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct BehaviorConfig {
    pub mouse: bool,
    pub hide_stopped_processes: bool,
    pub follow_logs: bool,
    #[schemars(range(min = 1, max = 1000))]
    pub log_preview_lines: usize,
    #[schemars(range(min = 1, max = 1_000_000))]
    pub log_history_lines: usize,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            mouse: true,
            hide_stopped_processes: false,
            follow_logs: true,
            log_preview_lines: 10,
            log_history_lines: 1000,
        }
    }
}

impl BehaviorConfig {
    fn validate(&self) -> Result<(), UserConfigError> {
        if !(1..=1000).contains(&self.log_preview_lines) {
            return validation("tui.behavior.log_preview_lines must be between 1 and 1000");
        }
        if self.log_history_lines < self.log_preview_lines || self.log_history_lines > 1_000_000 {
            return validation(
                "tui.behavior.log_history_lines must be at least log_preview_lines and at most 1000000",
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct KeybindingsConfig {
    #[schemars(range(min = 100, max = 5000))]
    pub sequence_timeout_ms: u64,
    #[schemars(schema_with = "main_keybindings_schema")]
    pub main: BTreeMap<String, Vec<String>>,
    #[schemars(schema_with = "process_search_keybindings_schema")]
    pub process_search: BTreeMap<String, Vec<String>>,
    #[schemars(schema_with = "logs_keybindings_schema")]
    pub logs: BTreeMap<String, Vec<String>>,
    #[schemars(schema_with = "log_search_keybindings_schema")]
    pub log_search: BTreeMap<String, Vec<String>>,
    #[schemars(schema_with = "prompt_keybindings_schema")]
    pub prompt: BTreeMap<String, Vec<String>>,
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            sequence_timeout_ms: 750,
            main: BTreeMap::new(),
            process_search: BTreeMap::new(),
            logs: BTreeMap::new(),
            log_search: BTreeMap::new(),
            prompt: BTreeMap::new(),
        }
    }
}

impl KeybindingsConfig {
    pub fn is_default(&self) -> bool {
        self.sequence_timeout_ms == 750
            && self.main.is_empty()
            && self.process_search.is_empty()
            && self.logs.is_empty()
            && self.log_search.is_empty()
            && self.prompt.is_empty()
    }

    pub fn resolve(&self) -> Result<Keymap, UserConfigError> {
        if !(100..=5000).contains(&self.sequence_timeout_ms) {
            return validation("tui.keybindings.sequence_timeout_ms must be between 100 and 5000");
        }
        let mut keymap = Keymap::defaults(self.sequence_timeout_ms);
        for (context, overrides) in [
            (KeyContext::Main, &self.main),
            (KeyContext::ProcessSearch, &self.process_search),
            (KeyContext::Logs, &self.logs),
            (KeyContext::LogSearch, &self.log_search),
            (KeyContext::Prompt, &self.prompt),
        ] {
            for (name, sequences) in overrides {
                let Some(action) = Action::parse(context, name) else {
                    return validation(format!(
                        "unknown action tui.keybindings.{}.{name}",
                        context.as_str()
                    ));
                };
                let mut resolved = Vec::with_capacity(sequences.len());
                for sequence in sequences {
                    resolved.push(KeySequence::parse(sequence).map_err(|message| {
                        UserConfigError::Validation {
                            message: format!(
                                "tui.keybindings.{}.{name}: {message}",
                                context.as_str()
                            ),
                        }
                    })?);
                }
                keymap.replace(context, action, resolved);
            }
        }
        keymap.validate()?;
        Ok(keymap)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyContext {
    Main,
    ProcessSearch,
    Logs,
    LogSearch,
    Prompt,
}

impl KeyContext {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::ProcessSearch => "process_search",
            Self::Logs => "logs",
            Self::LogSearch => "log_search",
            Self::Prompt => "prompt",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    MoveDown,
    MoveUp,
    HalfPageDown,
    HalfPageUp,
    Activate,
    Expand,
    Collapse,
    OpenLogs,
    Search,
    RestartProcess,
    StopProcess,
    ToggleStopped,
    Cancel,
    NextMatch,
    PreviousMatch,
    Accept,
    LineDown,
    LineUp,
    PageDown,
    PageUp,
    Top,
    Bottom,
    Copy,
    Back,
    Quit,
    StopManager,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MoveDown => "move_down",
            Self::MoveUp => "move_up",
            Self::HalfPageDown => "half_page_down",
            Self::HalfPageUp => "half_page_up",
            Self::Activate => "activate",
            Self::Expand => "expand",
            Self::Collapse => "collapse",
            Self::OpenLogs => "open_logs",
            Self::Search => "search",
            Self::RestartProcess => "restart_process",
            Self::StopProcess => "stop_process",
            Self::ToggleStopped => "toggle_stopped",
            Self::Cancel => "cancel",
            Self::NextMatch => "next_match",
            Self::PreviousMatch => "previous_match",
            Self::Accept => "accept",
            Self::LineDown => "line_down",
            Self::LineUp => "line_up",
            Self::PageDown => "page_down",
            Self::PageUp => "page_up",
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Copy => "copy",
            Self::Back => "back",
            Self::Quit => "quit",
            Self::StopManager => "stop_manager",
        }
    }

    fn parse(context: KeyContext, name: &str) -> Option<Self> {
        actions_for(context)
            .iter()
            .copied()
            .find(|action| action.as_str() == name)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::MoveDown | Self::MoveUp => "navigate",
            Self::HalfPageDown | Self::HalfPageUp => "half page",
            Self::Activate => "select",
            Self::Expand => "expand",
            Self::Collapse => "collapse",
            Self::OpenLogs => "logs",
            Self::Search => "search",
            Self::RestartProcess => "restart",
            Self::StopProcess => "stop",
            Self::ToggleStopped => "toggle stopped",
            Self::Cancel => "cancel",
            Self::NextMatch => "next",
            Self::PreviousMatch => "previous",
            Self::Accept => "accept",
            Self::LineDown | Self::LineUp => "line",
            Self::PageDown | Self::PageUp => "page",
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Copy => "copy",
            Self::Back => "back",
            Self::Quit => "quit",
            Self::StopManager => "stop manager",
        }
    }
}

fn actions_for(context: KeyContext) -> &'static [Action] {
    match context {
        KeyContext::Main => &[
            Action::MoveDown,
            Action::MoveUp,
            Action::HalfPageDown,
            Action::HalfPageUp,
            Action::Activate,
            Action::Expand,
            Action::Collapse,
            Action::OpenLogs,
            Action::Search,
            Action::RestartProcess,
            Action::StopProcess,
            Action::ToggleStopped,
            Action::Cancel,
        ],
        KeyContext::ProcessSearch => &[
            Action::NextMatch,
            Action::PreviousMatch,
            Action::Accept,
            Action::Cancel,
        ],
        KeyContext::Logs => &[
            Action::LineDown,
            Action::LineUp,
            Action::HalfPageDown,
            Action::HalfPageUp,
            Action::PageDown,
            Action::PageUp,
            Action::Top,
            Action::Bottom,
            Action::Search,
            Action::NextMatch,
            Action::PreviousMatch,
            Action::Copy,
            Action::Back,
        ],
        KeyContext::LogSearch => &[Action::Accept, Action::Cancel],
        KeyContext::Prompt => &[Action::Cancel, Action::Quit, Action::StopManager],
    }
}

struct KeySequenceSchema;

impl JsonSchema for KeySequenceSchema {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "KeySequence".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let pattern =
            format!("^(?:{KEY_CHORD_PATTERN})(?:[\\t\\n\\r ]+(?:{KEY_CHORD_PATTERN})){{0,3}}$");
        json_schema!({
            "type": "string",
            "pattern": pattern,
            "not": {
                "pattern": RESERVED_CTRL_C_PATTERN,
            },
        })
    }
}

struct ShellKeyChordSchema;

impl JsonSchema for ShellKeyChordSchema {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ShellKeyChord".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let pattern = format!("^(?:{KEY_CHORD_PATTERN})$");
        json_schema!({
            "type": "string",
            "pattern": pattern,
            "not": {
                "pattern": RESERVED_CTRL_C_PATTERN,
            },
        })
    }
}

fn shell_keybindings_schema(generator: &mut SchemaGenerator) -> Schema {
    let bindings_schema = json_schema!({
        "type": "array",
        "items": generator.subschema_for::<ShellKeyChordSchema>(),
        "uniqueItems": true,
    });
    let properties = ShellAction::ALL
        .into_iter()
        .map(|action| {
            (
                action.as_str().to_string(),
                serde_json::Value::from(bindings_schema.clone()),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json_schema!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
    })
}

fn action_bindings_schema(generator: &mut SchemaGenerator, context: KeyContext) -> Schema {
    let bindings_schema = json_schema!({
        "type": "array",
        "items": generator.subschema_for::<KeySequenceSchema>(),
        "uniqueItems": true,
    });
    let properties = actions_for(context)
        .iter()
        .map(|action| {
            (
                action.as_str().to_string(),
                serde_json::Value::from(bindings_schema.clone()),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json_schema!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
    })
}

fn main_keybindings_schema(generator: &mut SchemaGenerator) -> Schema {
    action_bindings_schema(generator, KeyContext::Main)
}

fn process_search_keybindings_schema(generator: &mut SchemaGenerator) -> Schema {
    action_bindings_schema(generator, KeyContext::ProcessSearch)
}

fn logs_keybindings_schema(generator: &mut SchemaGenerator) -> Schema {
    action_bindings_schema(generator, KeyContext::Logs)
}

fn log_search_keybindings_schema(generator: &mut SchemaGenerator) -> Schema {
    action_bindings_schema(generator, KeyContext::LogSearch)
}

fn prompt_keybindings_schema(generator: &mut SchemaGenerator) -> Schema {
    action_bindings_schema(generator, KeyContext::Prompt)
}

#[derive(Clone, Debug)]
pub struct Keymap {
    pub sequence_timeout_ms: u64,
    bindings: BTreeMap<KeyContext, BTreeMap<Action, Vec<KeySequence>>>,
}

impl Keymap {
    fn defaults(sequence_timeout_ms: u64) -> Self {
        let mut keymap = Self {
            sequence_timeout_ms,
            bindings: BTreeMap::new(),
        };
        for (context, action, keys) in default_bindings() {
            keymap.replace(
                context,
                action,
                keys.into_iter()
                    .map(|key| KeySequence::parse(key).unwrap())
                    .collect(),
            );
        }
        keymap
    }

    fn replace(&mut self, context: KeyContext, action: Action, keys: Vec<KeySequence>) {
        self.bindings.entry(context).or_default().insert(
            action,
            keys.into_iter()
                .map(KeySequence::normalized_for_tui)
                .collect(),
        );
    }

    fn validate(&self) -> Result<(), UserConfigError> {
        for (context, actions) in &self.bindings {
            let mut sequences: Vec<(&KeySequence, Action)> = Vec::new();
            for (action, keys) in actions {
                for sequence in keys {
                    for (existing, existing_action) in &sequences {
                        if sequence == *existing {
                            return validation(format!(
                                "tui.keybindings.{} assigns key sequence {:?} to both {} and {}",
                                context.as_str(),
                                sequence.label(),
                                existing_action.as_str(),
                                action.as_str()
                            ));
                        }
                        if sequence.is_prefix_of(existing) || existing.is_prefix_of(sequence) {
                            return validation(format!(
                                "tui.keybindings.{} contains prefix-ambiguous key sequences {:?} and {:?}",
                                context.as_str(),
                                existing.label(),
                                sequence.label()
                            ));
                        }
                    }
                    sequences.push((sequence, *action));
                }
            }
        }
        Ok(())
    }

    pub fn bindings(&self, context: KeyContext, action: Action) -> &[KeySequence] {
        self.bindings
            .get(&context)
            .and_then(|actions| actions.get(&action))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn match_sequence(&self, context: KeyContext, sequence: &[KeyChord]) -> KeyMatch {
        let mut prefix = false;
        if let Some(actions) = self.bindings.get(&context) {
            for (action, bindings) in actions {
                for binding in bindings {
                    if binding.chords == sequence {
                        return KeyMatch::Action(*action);
                    }
                    if sequence.len() < binding.chords.len()
                        && binding.chords[..sequence.len()] == *sequence
                    {
                        prefix = true;
                    }
                }
            }
        }
        if prefix {
            KeyMatch::Prefix
        } else {
            KeyMatch::None
        }
    }

    pub fn hint(&self, context: KeyContext, action: Action) -> Option<String> {
        self.key_label(context, action, false)
            .map(|key| format!("{key} {}", action.label()))
    }

    pub fn key_label(&self, context: KeyContext, action: Action, compact: bool) -> Option<String> {
        let bindings = self.bindings(context, action);
        let label = if compact {
            bindings
                .iter()
                .map(KeySequence::compact_label)
                .min_by_key(|label| display_width(label))?
        } else {
            bindings.first()?.label()
        };
        Some(label)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyMatch {
    Action(Action),
    Prefix,
    None,
}

#[derive(Clone, Debug, Default)]
pub struct KeySequenceState {
    pending: Vec<KeyChord>,
    deadline: Option<Instant>,
}

impl KeySequenceState {
    pub fn input(&mut self, keymap: &Keymap, context: KeyContext, event: &KeyEvent) -> KeyMatch {
        self.input_key(keymap, context, event.code, event.modifiers)
    }

    pub fn input_key(
        &mut self,
        keymap: &Keymap,
        context: KeyContext,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> KeyMatch {
        self.expire();
        let Some(chord) = KeyChord::from_key(code, modifiers) else {
            self.clear();
            return KeyMatch::None;
        };
        self.pending.push(chord.clone());
        let matched = keymap.match_sequence(context, &self.pending);
        match matched {
            KeyMatch::Action(_) => self.clear(),
            KeyMatch::Prefix => {
                self.deadline =
                    Some(Instant::now() + Duration::from_millis(keymap.sequence_timeout_ms));
            }
            KeyMatch::None if self.pending.len() > 1 => {
                self.pending.clear();
                self.pending.push(chord);
                let retried = keymap.match_sequence(context, &self.pending);
                match retried {
                    KeyMatch::Action(_) => self.clear(),
                    KeyMatch::Prefix => {
                        self.deadline = Some(
                            Instant::now() + Duration::from_millis(keymap.sequence_timeout_ms),
                        );
                    }
                    KeyMatch::None => self.clear(),
                }
                return retried;
            }
            KeyMatch::None => self.clear(),
        }
        matched
    }

    pub fn pending_label(&self) -> Option<String> {
        (!self.pending.is_empty()).then(|| {
            self.pending
                .iter()
                .map(KeyChord::label)
                .collect::<Vec<_>>()
                .join(" ")
        })
    }

    pub fn remaining_timeout(&self) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    pub fn expire(&mut self) -> bool {
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.clear();
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.deadline = None;
    }
}

fn default_bindings() -> Vec<(KeyContext, Action, Vec<&'static str>)> {
    use Action::*;
    use KeyContext::*;
    vec![
        (Main, MoveDown, vec!["down", "j"]),
        (Main, MoveUp, vec!["up", "k"]),
        (Main, HalfPageDown, vec!["ctrl+d"]),
        (Main, HalfPageUp, vec!["ctrl+u"]),
        (Main, Activate, vec!["enter"]),
        (Main, Expand, vec!["right", "l"]),
        (Main, Collapse, vec!["left", "h"]),
        (Main, OpenLogs, vec!["ctrl+e"]),
        (Main, Search, vec!["/"]),
        (Main, RestartProcess, vec!["ctrl+r"]),
        (Main, StopProcess, vec!["ctrl+x"]),
        (Main, ToggleStopped, vec!["ctrl+h"]),
        (Main, Cancel, vec!["esc"]),
        (ProcessSearch, NextMatch, vec!["down"]),
        (ProcessSearch, PreviousMatch, vec!["up"]),
        (ProcessSearch, Accept, vec!["enter"]),
        (ProcessSearch, Cancel, vec!["esc"]),
        (Logs, LineDown, vec!["down", "j"]),
        (Logs, LineUp, vec!["up", "k"]),
        (Logs, HalfPageDown, vec!["ctrl+d"]),
        (Logs, HalfPageUp, vec!["ctrl+u"]),
        (Logs, PageDown, vec!["page_down", "space", "ctrl+f"]),
        (Logs, PageUp, vec!["page_up", "ctrl+b"]),
        (Logs, Top, vec!["home", "g"]),
        (Logs, Bottom, vec!["end", "shift+g"]),
        (Logs, Search, vec!["/"]),
        (Logs, NextMatch, vec!["n"]),
        (Logs, PreviousMatch, vec!["shift+n"]),
        (Logs, Copy, vec!["y"]),
        (Logs, Back, vec!["q", "esc", "ctrl+e"]),
        (LogSearch, Accept, vec!["enter"]),
        (LogSearch, Cancel, vec!["esc"]),
        (Prompt, Cancel, vec!["c", "esc"]),
        (Prompt, Quit, vec!["q"]),
        (Prompt, StopManager, vec!["s"]),
    ]
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeySequence {
    pub chords: Vec<KeyChord>,
}

impl KeySequence {
    pub fn parse(input: &str) -> Result<Self, String> {
        let chords = input
            .split_ascii_whitespace()
            .map(KeyChord::parse)
            .collect::<Result<Vec<_>, _>>()?;
        if chords.is_empty() {
            return Err("key sequence cannot be empty".to_string());
        }
        if chords.len() > 4 {
            return Err("key sequence cannot contain more than four chords".to_string());
        }
        if chords.iter().any(KeyChord::is_ctrl_c) {
            return Err("ctrl+c is reserved for emergency interrupt and copy".to_string());
        }
        Ok(Self { chords })
    }

    fn normalized_for_tui(mut self) -> Self {
        for chord in &mut self.chords {
            chord.normalize_for_tui();
        }
        self
    }

    fn is_prefix_of(&self, other: &Self) -> bool {
        self.chords.len() < other.chords.len() && other.chords[..self.chords.len()] == self.chords
    }

    pub fn label(&self) -> String {
        self.chords
            .iter()
            .map(KeyChord::label)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn compact_label(&self) -> String {
        self.chords
            .iter()
            .map(KeyChord::compact_label)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyChord {
    pub code: KeyCodeSpec,
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyChord {
    pub fn from_event(event: &KeyEvent) -> Option<Self> {
        Self::from_key(event.code, event.modifiers)
    }

    pub fn from_key(code: KeyCode, modifiers: KeyModifiers) -> Option<Self> {
        let shifted_character =
            matches!(code, KeyCode::Char(character) if character.is_ascii_uppercase());
        let code = KeyCodeSpec::from_key_code(code)?;
        let shift = modifiers.contains(KeyModifiers::SHIFT) || shifted_character;
        let mut chord = Self {
            code,
            control: modifiers.contains(KeyModifiers::CONTROL),
            alt: modifiers.contains(KeyModifiers::ALT),
            shift,
        };
        chord.normalize_for_tui();
        Some(chord)
    }

    pub fn parse(input: &str) -> Result<Self, String> {
        if input.is_empty() {
            return Err("key chord cannot be empty".to_string());
        }
        let parts = input.split('+').collect::<Vec<_>>();
        let Some(key) = parts.last().copied() else {
            return Err("key chord cannot be empty".to_string());
        };
        if key.is_empty() {
            return Err(format!("invalid key chord {input:?}"));
        }
        let mut control = false;
        let mut alt = false;
        let mut shift = false;
        for modifier in &parts[..parts.len() - 1] {
            match *modifier {
                "ctrl" => control = true,
                "alt" => alt = true,
                "shift" => shift = true,
                _ => return Err(format!("unknown modifier {modifier:?} in {input:?}")),
            }
        }
        let code = KeyCodeSpec::parse(key)?;
        Ok(Self {
            code,
            control,
            alt,
            shift,
        })
    }

    fn is_ctrl_c(&self) -> bool {
        self.control && !self.alt && matches!(self.code, KeyCodeSpec::Char('c'))
    }

    fn normalize_for_tui(&mut self) {
        if self.code == KeyCodeSpec::BackTab || (self.code == KeyCodeSpec::Tab && self.shift) {
            self.code = KeyCodeSpec::BackTab;
            self.shift = false;
        }
        if !self.shift {
            return;
        }
        let KeyCodeSpec::Char(character) = self.code else {
            return;
        };
        if let Some(shifted) = normalized_shifted_ascii_symbol(character) {
            self.code = KeyCodeSpec::Char(shifted);
            self.shift = false;
        }
    }

    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.control {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        parts.push(match self.code {
            KeyCodeSpec::Char(character) if self.control || self.alt || self.shift => {
                character.to_ascii_uppercase().to_string()
            }
            _ => self.code.label(),
        });
        parts.join("+")
    }

    fn compact_label(&self) -> String {
        if self.control
            && !self.alt
            && !self.shift
            && let KeyCodeSpec::Char(character) = self.code
        {
            format!("^{}", character.to_ascii_uppercase())
        } else {
            self.label()
        }
    }

    fn to_shell_chord(&self) -> ShellKeyChord {
        let code = match self.code {
            KeyCodeSpec::Char(character) => ShellKeyCode::Char(character),
            KeyCodeSpec::Enter => ShellKeyCode::Enter,
            KeyCodeSpec::Esc => ShellKeyCode::Esc,
            KeyCodeSpec::Backspace => ShellKeyCode::Backspace,
            KeyCodeSpec::Delete => ShellKeyCode::Delete,
            KeyCodeSpec::Insert => ShellKeyCode::Insert,
            KeyCodeSpec::Up => ShellKeyCode::Up,
            KeyCodeSpec::Down => ShellKeyCode::Down,
            KeyCodeSpec::Left => ShellKeyCode::Left,
            KeyCodeSpec::Right => ShellKeyCode::Right,
            KeyCodeSpec::Home => ShellKeyCode::Home,
            KeyCodeSpec::End => ShellKeyCode::End,
            KeyCodeSpec::PageUp => ShellKeyCode::PageUp,
            KeyCodeSpec::PageDown => ShellKeyCode::PageDown,
            KeyCodeSpec::Tab => ShellKeyCode::Tab,
            KeyCodeSpec::BackTab => ShellKeyCode::BackTab,
            KeyCodeSpec::Space => ShellKeyCode::Space,
            KeyCodeSpec::Function(number) => ShellKeyCode::Function(number),
        };
        ShellKeyChord::new(code, self.control, self.alt, self.shift)
    }
}

pub(crate) fn is_emergency_interrupt(code: KeyCode, modifiers: KeyModifiers) -> bool {
    code == KeyCode::Char('c')
        && modifiers.contains(KeyModifiers::CONTROL)
        && !modifiers.contains(KeyModifiers::ALT)
}

fn normalized_shifted_ascii_symbol(character: char) -> Option<char> {
    match character {
        '`' | '~' => Some('~'),
        '1' | '!' => Some('!'),
        '2' | '@' => Some('@'),
        '3' | '#' => Some('#'),
        '4' | '$' => Some('$'),
        '5' | '%' => Some('%'),
        '6' | '^' => Some('^'),
        '7' | '&' => Some('&'),
        '8' | '*' => Some('*'),
        '9' | '(' => Some('('),
        '0' | ')' => Some(')'),
        '-' | '_' => Some('_'),
        '=' | '+' => Some('+'),
        '[' | '{' => Some('{'),
        ']' | '}' => Some('}'),
        '\\' | '|' => Some('|'),
        ';' | ':' => Some(':'),
        '\'' | '"' => Some('"'),
        ',' | '<' => Some('<'),
        '.' | '>' => Some('>'),
        '/' | '?' => Some('?'),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyCodeSpec {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Delete,
    Insert,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    Space,
    Function(u8),
}

impl KeyCodeSpec {
    fn from_key_code(code: KeyCode) -> Option<Self> {
        Some(match code {
            KeyCode::Char(' ') => Self::Space,
            KeyCode::Char(character) => Self::Char(character.to_ascii_lowercase()),
            KeyCode::Enter => Self::Enter,
            KeyCode::Esc => Self::Esc,
            KeyCode::Backspace => Self::Backspace,
            KeyCode::Delete => Self::Delete,
            KeyCode::Insert => Self::Insert,
            KeyCode::Up => Self::Up,
            KeyCode::Down => Self::Down,
            KeyCode::Left => Self::Left,
            KeyCode::Right => Self::Right,
            KeyCode::Home => Self::Home,
            KeyCode::End => Self::End,
            KeyCode::PageUp => Self::PageUp,
            KeyCode::PageDown => Self::PageDown,
            KeyCode::Tab => Self::Tab,
            KeyCode::BackTab => Self::BackTab,
            KeyCode::F(number) => Self::Function(number),
            _ => return None,
        })
    }

    fn label(&self) -> String {
        match self {
            Self::Char(character) => character.to_string(),
            Self::Enter => "Enter".to_string(),
            Self::Esc => "Esc".to_string(),
            Self::Backspace => "Backspace".to_string(),
            Self::Delete => "Delete".to_string(),
            Self::Insert => "Insert".to_string(),
            Self::Up => "↑".to_string(),
            Self::Down => "↓".to_string(),
            Self::Left => "←".to_string(),
            Self::Right => "→".to_string(),
            Self::Home => "Home".to_string(),
            Self::End => "End".to_string(),
            Self::PageUp => "PageUp".to_string(),
            Self::PageDown => "PageDown".to_string(),
            Self::Tab => "Tab".to_string(),
            Self::BackTab => "Shift+Tab".to_string(),
            Self::Space => "Space".to_string(),
            Self::Function(number) => format!("F{number}"),
        }
    }

    fn parse(input: &str) -> Result<Self, String> {
        let code = match input {
            "enter" => Self::Enter,
            "esc" | "escape" => Self::Esc,
            "backspace" => Self::Backspace,
            "delete" => Self::Delete,
            "insert" => Self::Insert,
            "up" => Self::Up,
            "down" => Self::Down,
            "left" => Self::Left,
            "right" => Self::Right,
            "home" => Self::Home,
            "end" => Self::End,
            "page_up" | "page-up" => Self::PageUp,
            "page_down" | "page-down" => Self::PageDown,
            "tab" => Self::Tab,
            "back_tab" | "back-tab" => Self::BackTab,
            "space" => Self::Space,
            value if value.starts_with('f') && (2..=3).contains(&value.len()) => {
                let number = value[1..]
                    .parse::<u8>()
                    .map_err(|_| format!("unknown key {value:?}"))?;
                if !(1..=24).contains(&number) {
                    return Err("function keys must be between f1 and f24".to_string());
                }
                Self::Function(number)
            }
            value => {
                let mut chars = value.chars();
                let Some(character) = chars.next() else {
                    return Err("key cannot be empty".to_string());
                };
                if chars.next().is_some() {
                    return Err(format!("unknown key {value:?}"));
                }
                if character.is_ascii_uppercase() {
                    return Err(format!(
                        "uppercase character keys are ambiguous; use lowercase {:?} and add shift+ explicitly when needed",
                        character.to_ascii_lowercase()
                    ));
                }
                Self::Char(character)
            }
        };
        Ok(code)
    }
}

fn valid_name(value: &str, allow_dot: bool) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') || allow_dot && byte == b'.'
        })
}

fn validate_single_line(value: &str, max_chars: usize, path: &str) -> Result<(), UserConfigError> {
    if value.chars().any(char::is_control) {
        return validation(format!("{path} must be a single-line value"));
    }
    if value.chars().count() > max_chars {
        return validation(format!("{path} cannot exceed {max_chars} characters"));
    }
    Ok(())
}

fn display_width(value: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(value)
}

fn validation<T>(message: impl Into<String>) -> Result<T, UserConfigError> {
    Err(UserConfigError::Validation {
        message: message.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips() {
        let config = UserConfig::default();
        let serialized = config.to_yaml().unwrap();
        assert!(serialized.starts_with(
            "# yaml-language-server: $schema=https://devenv.sh/devenv.user.schema.json\n\n"
        ));
        assert!(!serialized.contains("\"$schema\""));
        assert!(serialized.contains("\nshell:\n  keybindings: {}\n"));
        let parsed = UserConfig::from_yaml("config.yaml", serialized).unwrap();
        assert_eq!(parsed.version, USER_CONFIG_VERSION);
        assert_eq!(parsed.tui.viewport, ViewportPlacement::Inline);
        assert_eq!(parsed.tui.statusline.position, StatuslinePosition::Inline);
    }

    #[test]
    fn schema_exposes_static_runtime_constraints() {
        let schema = serde_json::to_value(schemars::schema_for!(UserConfig)).unwrap();

        assert!(schema.pointer("/properties/$schema").is_none());
        assert_eq!(
            schema.pointer("/properties/version/minimum"),
            Some(&1.into())
        );
        assert_eq!(
            schema.pointer("/properties/version/maximum"),
            Some(&1.into())
        );
        assert_eq!(
            schema.pointer("/$defs/KeybindingsConfig/properties/sequence_timeout_ms/minimum"),
            Some(&100.into())
        );
        assert_eq!(
            schema.pointer("/$defs/KeybindingsConfig/properties/sequence_timeout_ms/maximum"),
            Some(&5000.into())
        );
        assert_eq!(
            schema.pointer("/$defs/BehaviorConfig/properties/log_preview_lines/minimum"),
            Some(&1.into())
        );
        assert_eq!(
            schema.pointer("/$defs/BehaviorConfig/properties/log_preview_lines/maximum"),
            Some(&1000.into())
        );
        assert_eq!(
            schema.pointer("/$defs/BehaviorConfig/properties/log_history_lines/maximum"),
            Some(&1_000_000.into())
        );
        assert_eq!(
            schema.pointer("/$defs/KeybindingsConfig/properties/logs/additionalProperties"),
            Some(&false.into())
        );
        assert!(
            schema
                .pointer("/$defs/KeybindingsConfig/properties/logs/properties/back")
                .is_some()
        );
        assert!(
            schema
                .pointer("/$defs/KeybindingsConfig/properties/main/properties/back")
                .is_none()
        );
        assert!(
            schema
                .pointer("/$defs/KeybindingsConfig/properties/logs/properties/back/items/$ref")
                .is_some()
        );
        assert!(schema.pointer("/$defs/KeySequence/pattern").is_some());
        assert!(schema.pointer("/$defs/KeySequence/not/pattern").is_some());
        assert_eq!(
            schema.pointer("/$defs/ShellPreferences/properties/keybindings/additionalProperties"),
            Some(&false.into())
        );
        assert!(
            schema
                .pointer(
                    "/$defs/ShellPreferences/properties/keybindings/properties/reload/items/$ref"
                )
                .is_some()
        );
        assert!(schema.pointer("/$defs/ShellKeyChord/pattern").is_some());
        assert_eq!(
            schema.pointer("/$defs/StatuslinePosition/enum"),
            Some(&serde_json::json!(["top", "bottom", "inline"]))
        );
        assert_eq!(
            schema.pointer("/$defs/ViewportPlacement/enum"),
            Some(&serde_json::json!(["inline", "top"]))
        );
    }

    #[test]
    fn parses_premium_example() {
        let config = UserConfig::from_yaml(
            "config.yaml",
            r##"
# yaml-language-server: $schema=https://devenv.sh/devenv.user.schema.json

version: 1
tui:
  viewport: top
  theme:
    preset: devenv
    palette:
      profile: "#cba6f7"
    styles:
      statusline.profiles:
        foreground: profile
        modifiers: [bold]
  statusline:
    position: top
    layouts:
      main:
        left: [profiles, summary]
        center: [project]
        right: [key_hints]
    components:
      profiles:
        format: "profile {profiles}"
        compact_format: "{profiles}"
        priority: 80
  keybindings:
    sequence_timeout_ms: 750
    logs:
      top: [home, "g g"]
"##
            .to_string(),
        )
        .unwrap();
        assert_eq!(config.tui.viewport, ViewportPlacement::Top);
        assert_eq!(config.tui.statusline.position, StatuslinePosition::Top);
        assert_eq!(config.tui.statusline.layouts.main.left[0], "profiles");
        assert_eq!(
            config
                .tui
                .keybindings
                .resolve()
                .unwrap()
                .bindings(KeyContext::Logs, Action::Top)[1]
                .chords
                .len(),
            2
        );
    }

    #[test]
    fn rejects_unknown_fields_with_a_span() {
        let error = UserConfig::from_yaml(
            "config.yaml",
            "version: 1\ntui:\n  unknown: true\n".to_string(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            UserConfigError::Parse { span: Some(_), .. }
        ));
    }

    #[test]
    fn rejects_duplicate_mapping_keys_with_a_span() {
        let error = UserConfig::from_yaml(
            "config.yaml",
            "version: 1\ntui:\n  theme:\n    palette:\n      accent: red\n      accent: blue\n"
                .to_string(),
        )
        .unwrap_err();
        match error {
            UserConfigError::Parse {
                span: Some(_),
                message,
                ..
            } => assert!(message.contains("duplicate entry with key \"accent\"")),
            error => panic!("expected duplicate-key parse error with a span, got {error:?}"),
        }
    }

    #[test]
    fn resolves_supported_colors() {
        assert_eq!(
            ColorSpec("ansi:220".to_string()).parse_direct().unwrap(),
            Color::AnsiValue(220)
        );
        assert_eq!(
            ColorSpec("#708a58".to_string()).parse_direct().unwrap(),
            Color::Rgb {
                r: 112,
                g: 138,
                b: 88
            }
        );
        assert_eq!(
            ColorSpec("default".to_string()).parse_direct().unwrap(),
            Color::Reset
        );
    }

    #[test]
    fn rejects_conflicting_bindings() {
        let mut config = KeybindingsConfig::default();
        config
            .main
            .insert("move_down".to_string(), vec!["x".to_string()]);
        config
            .main
            .insert("move_up".to_string(), vec!["x".to_string()]);
        assert_eq!(
            config.resolve().unwrap_err().to_string(),
            "invalid user configuration: tui.keybindings.main assigns key sequence \"x\" to both move_down and move_up"
        );
    }

    #[test]
    fn resolves_shell_keybinding_overrides_and_unbindings() {
        let config = UserConfig::from_yaml(
            "config.yaml",
            "version: 1\nshell:\n  keybindings:\n    toggle_pause: [f12]\n    toggle_error: []\n    reload: [alt+r, f5]\n"
                .to_string(),
        )
        .unwrap();
        let keybindings = config.shell.resolve().unwrap();
        assert_eq!(
            keybindings.key_label(ShellAction::TogglePause, false),
            Some("F12".to_string())
        );
        assert!(keybindings.bindings(ShellAction::ToggleError).is_empty());
        assert_eq!(keybindings.reload_bindings("bash").len(), 2);
    }

    #[test]
    fn all_shell_keybindings_can_be_unbound() {
        let defaults = ShellKeybindings::default();
        let intercepted_inputs = ShellAction::INTERCEPTED
            .into_iter()
            .flat_map(|action| defaults.bindings(action))
            .map(ShellKeyChord::terminal_bytes)
            .collect::<Vec<_>>();
        let config = UserConfig::from_yaml(
            "config.yaml",
            "version: 1\nshell:\n  keybindings:\n    toggle_pause: []\n    list_watched_files: []\n    toggle_error: []\n    reload: []\n".to_string(),
        )
        .unwrap();
        let keybindings = config.shell.resolve().unwrap();

        for action in ShellAction::ALL {
            assert!(keybindings.bindings(action).is_empty());
        }
        for input in intercepted_inputs {
            assert_eq!(keybindings.action_for_input(&input), None);
        }
        for dialect in ["bash", "fish", "nu", "zsh"] {
            assert!(keybindings.reload_bindings(dialect).is_empty());
        }
    }

    #[test]
    fn rejects_shell_keybinding_sequences() {
        let mut config = ShellPreferences::default();
        config
            .keybindings
            .insert("toggle_pause".to_string(), vec!["g g".to_string()]);
        assert_eq!(
            config.resolve().unwrap_err().to_string(),
            "invalid user configuration: shell.keybindings.toggle_pause: expected exactly one key chord"
        );
    }

    #[test]
    fn rejects_shell_keybindings_with_conflicting_terminal_encodings() {
        let mut config = ShellPreferences::default();
        config
            .keybindings
            .insert("toggle_pause".to_string(), vec!["ctrl+d".to_string()]);
        config.keybindings.insert(
            "list_watched_files".to_string(),
            vec!["ctrl+shift+d".to_string()],
        );
        assert_eq!(
            config.resolve().unwrap_err().to_string(),
            "invalid user configuration: shell.keybindings assigns \"Ctrl+D / Ctrl+Shift+D\" to both \"toggle_pause\" and \"list_watched_files\""
        );
    }

    #[test]
    fn rejects_shell_keybindings_with_prefix_ambiguous_terminal_encodings() {
        let mut config = ShellPreferences::default();
        config
            .keybindings
            .insert("toggle_pause".to_string(), vec!["esc".to_string()]);
        assert_eq!(
            config.resolve().unwrap_err().to_string(),
            "invalid user configuration: shell.keybindings assigns \"Esc / Ctrl+Alt+W\" to both \"toggle_pause\" and \"list_watched_files\""
        );
    }

    #[test]
    fn rejects_prefix_ambiguous_bindings() {
        let mut config = KeybindingsConfig::default();
        config
            .main
            .insert("move_down".to_string(), vec!["x".to_string()]);
        config
            .main
            .insert("move_up".to_string(), vec!["x x".to_string()]);
        assert_eq!(
            config.resolve().unwrap_err().to_string(),
            "invalid user configuration: tui.keybindings.main contains prefix-ambiguous key sequences \"x\" and \"x x\""
        );
    }

    #[test]
    fn rejects_ambiguous_uppercase_character_bindings() {
        let mut config = KeybindingsConfig::default();
        config
            .main
            .insert("open_logs".to_string(), vec!["ctrl+E".to_string()]);
        assert!(
            config
                .resolve()
                .unwrap_err()
                .to_string()
                .contains("uppercase character keys are ambiguous")
        );
    }

    #[test]
    fn dispatches_shifted_letters_across_terminal_encodings() {
        let keymap = KeybindingsConfig::default().resolve().unwrap();
        let mut state = KeySequenceState::default();

        assert_eq!(
            state.input_key(
                &keymap,
                KeyContext::Logs,
                KeyCode::Char('G'),
                KeyModifiers::NONE
            ),
            KeyMatch::Action(Action::Bottom)
        );
        assert_eq!(
            state.input_key(
                &keymap,
                KeyContext::Logs,
                KeyCode::Char('g'),
                KeyModifiers::SHIFT
            ),
            KeyMatch::Action(Action::Bottom)
        );
    }

    #[test]
    fn dispatches_shifted_tab_across_terminal_encodings() {
        for binding in ["shift+tab", "back_tab", "shift+back_tab", "back-tab"] {
            for modifiers in [KeyModifiers::NONE, KeyModifiers::CONTROL, KeyModifiers::ALT] {
                let prefix = if modifiers == KeyModifiers::CONTROL {
                    "ctrl+"
                } else if modifiers == KeyModifiers::ALT {
                    "alt+"
                } else {
                    ""
                };
                let mut config = KeybindingsConfig::default();
                config
                    .main
                    .insert("move_up".to_string(), vec![format!("{prefix}{binding}")]);
                let keymap = config.resolve().unwrap();
                let mut state = KeySequenceState::default();
                for (code, shift) in [
                    (KeyCode::Tab, KeyModifiers::SHIFT),
                    (KeyCode::BackTab, KeyModifiers::SHIFT),
                    (KeyCode::BackTab, KeyModifiers::NONE),
                ] {
                    assert_eq!(
                        state.input_key(&keymap, KeyContext::Main, code, modifiers | shift),
                        KeyMatch::Action(Action::MoveUp),
                        "{prefix}{binding} with {code:?} and {shift:?}"
                    );
                }
                assert_eq!(
                    state.input_key(&keymap, KeyContext::Main, KeyCode::Tab, modifiers),
                    KeyMatch::None
                );
            }
        }
    }

    #[test]
    fn rejects_equivalent_tab_bindings_and_sequence_prefixes() {
        for binding in ["back_tab", "shift+back-tab", "back_tab x"] {
            let mut config = KeybindingsConfig::default();
            config
                .main
                .insert("move_up".to_string(), vec!["shift+tab".to_string()]);
            config
                .main
                .insert("move_down".to_string(), vec![binding.to_string()]);
            assert!(config.resolve().is_err(), "{binding}");
        }
    }

    #[test]
    fn dispatches_shifted_symbols_as_terminal_characters() {
        let mut config = KeybindingsConfig::default();
        config
            .main
            .insert("search".to_string(), vec!["shift+1".to_string()]);
        let keymap = config.resolve().unwrap();
        let mut state = KeySequenceState::default();

        assert_eq!(
            state.input_key(
                &keymap,
                KeyContext::Main,
                KeyCode::Char('!'),
                KeyModifiers::NONE
            ),
            KeyMatch::Action(Action::Search)
        );
        assert_eq!(
            state.input_key(
                &keymap,
                KeyContext::Main,
                KeyCode::Char('1'),
                KeyModifiers::SHIFT
            ),
            KeyMatch::Action(Action::Search)
        );
        assert_eq!(
            state.input_key(
                &keymap,
                KeyContext::Main,
                KeyCode::Char('!'),
                KeyModifiers::SHIFT
            ),
            KeyMatch::Action(Action::Search)
        );
    }

    #[test]
    fn ctrl_alt_c_dispatches_without_becoming_an_emergency_interrupt() {
        let mut config = KeybindingsConfig::default();
        config
            .main
            .insert("search".to_string(), vec!["ctrl+alt+c".to_string()]);
        let keymap = config.resolve().unwrap();
        let mut state = KeySequenceState::default();
        let event = KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );

        assert!(!is_emergency_interrupt(event.code, event.modifiers));
        assert_eq!(
            state.input(&keymap, KeyContext::Main, &event),
            KeyMatch::Action(Action::Search)
        );
        assert!(is_emergency_interrupt(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        ));
    }

    #[test]
    fn log_search_editing_leaves_match_keys_as_text_input() {
        let keymap = KeybindingsConfig::default().resolve().unwrap();
        let mut state = KeySequenceState::default();

        assert_eq!(
            state.input_key(
                &keymap,
                KeyContext::LogSearch,
                KeyCode::Char('n'),
                KeyModifiers::NONE
            ),
            KeyMatch::None
        );
        assert_eq!(
            state.input_key(
                &keymap,
                KeyContext::LogSearch,
                KeyCode::Char('N'),
                KeyModifiers::SHIFT
            ),
            KeyMatch::None
        );
        assert_eq!(
            state.input_key(
                &keymap,
                KeyContext::Logs,
                KeyCode::Char('n'),
                KeyModifiers::NONE
            ),
            KeyMatch::Action(Action::NextMatch)
        );
    }

    #[test]
    fn rejects_unknown_status_components() {
        let mut config = StatuslineConfig::default();
        config.layouts.main.left.push("missing".to_string());
        assert!(config.validate(&BTreeMap::new()).is_err());
    }

    #[test]
    fn validates_component_format_variables() {
        let mut config = StatuslineConfig::default();
        config.components.insert(
            "profiles".to_string(),
            StatusComponentConfig {
                format: Some("{unknown}".to_string()),
                ..StatusComponentConfig::default()
            },
        );
        assert!(config.validate(&BTreeMap::new()).is_err());
    }

    #[test]
    fn dispatches_multi_key_sequences_and_retries_the_last_chord() {
        let mut config = KeybindingsConfig::default();
        config
            .logs
            .insert("top".to_string(), vec!["g g".to_string()]);
        let keymap = config.resolve().unwrap();
        let mut state = KeySequenceState::default();
        let g = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
        assert_eq!(state.input(&keymap, KeyContext::Logs, &g), KeyMatch::Prefix);
        assert_eq!(state.pending_label().as_deref(), Some("g"));
        state.deadline = Some(Instant::now() - Duration::from_millis(1));
        assert!(state.expire());
        assert!(state.pending_label().is_none());
        assert_eq!(state.input(&keymap, KeyContext::Logs, &g), KeyMatch::Prefix);
        assert_eq!(
            state.input(&keymap, KeyContext::Logs, &g),
            KeyMatch::Action(Action::Top)
        );
        assert!(state.pending_label().is_none());

        assert_eq!(state.input(&keymap, KeyContext::Logs, &g), KeyMatch::Prefix);
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(
            state.input(&keymap, KeyContext::Logs, &down),
            KeyMatch::Action(Action::LineDown)
        );
    }

    #[test]
    fn empty_binding_list_disables_defaults() {
        let mut config = KeybindingsConfig::default();
        config.main.insert("move_down".to_string(), vec![]);
        let keymap = config.resolve().unwrap();
        assert!(
            keymap
                .bindings(KeyContext::Main, Action::MoveDown)
                .is_empty()
        );
        let mut state = KeySequenceState::default();
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(
            state.input(&keymap, KeyContext::Main, &down),
            KeyMatch::None
        );
    }

    #[test]
    fn compact_key_labels_prefer_the_shortest_binding() {
        let keymap = KeybindingsConfig::default().resolve().unwrap();

        assert_eq!(
            keymap.key_label(KeyContext::Logs, Action::PageDown, true),
            Some("^F".to_string())
        );
        assert_eq!(
            keymap.key_label(KeyContext::Logs, Action::Top, true),
            Some("g".to_string())
        );
        assert_eq!(
            keymap.key_label(KeyContext::Logs, Action::PageDown, false),
            Some("PageDown".to_string())
        );
    }

    #[test]
    fn component_styles_can_use_custom_palette_entries() {
        let config = UserConfig::from_yaml(
            "config.yaml",
            r##"
version: 1
tui:
  theme:
    palette:
      accent: "#123456"
  statusline:
    components:
      project:
        style:
          foreground: accent
"##
            .to_string(),
        );
        assert!(config.is_ok());
    }
}
