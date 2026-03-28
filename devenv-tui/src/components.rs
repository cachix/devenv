//! Reusable UI components for the TUI

use crate::model::{Activity, ActivityVariant, NixActivityState};
use human_repr::{HumanCount, HumanThroughput};
use iocraft::prelude::*;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

// Import shared UI constants from devenv-shell
pub use devenv_shell::{
    CHECKMARK, COLOR_ACTIVE, COLOR_ACTIVE_NESTED, COLOR_COMPLETED, COLOR_FAILED, COLOR_HIERARCHY,
    COLOR_INFO, COLOR_INTERACTIVE, COLOR_SECONDARY, SPINNER_FRAMES, SPINNER_INTERVAL_MS, XMARK,
};

/// Self-animating spinner component.
/// Manages its own animation state and only re-renders itself.
#[derive(Default, Props)]
pub struct SpinnerProps {
    pub color: Option<Color>,
}

#[cfg(feature = "deterministic-tui")]
#[component]
pub fn Spinner(_hooks: Hooks, props: &SpinnerProps) -> impl Into<AnyElement<'static>> {
    let color = props.color.unwrap_or(COLOR_ACTIVE);

    element! {
        Text(content: SPINNER_FRAMES[0], color: color)
    }
}

#[cfg(not(feature = "deterministic-tui"))]
#[component]
pub fn Spinner(mut hooks: Hooks, props: &SpinnerProps) -> impl Into<AnyElement<'static>> {
    let mut frame = hooks.use_state(|| 0usize);
    let color = props.color.unwrap_or(COLOR_ACTIVE);

    hooks.use_future(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(SPINNER_INTERVAL_MS)).await;
            let Some(val) = frame.try_get() else {
                break;
            };
            frame.set((val + 1) % SPINNER_FRAMES.len());
        }
    });

    element! {
        Text(content: SPINNER_FRAMES[frame.get()], color: color)
    }
}

/// Reusable status indicator component.
/// Renders completion status: ✓ for success, ✗ for failure, spinner or space for in-progress.
#[derive(Default, Props)]
pub struct StatusIndicatorProps {
    /// Completion state: None = active, Some(true) = success, Some(false) = failed
    pub completed: Option<bool>,
    /// Whether to show a spinner when active (None). If false, shows a space.
    pub show_spinner: bool,
}

#[component]
pub fn StatusIndicator(
    _hooks: Hooks,
    props: &StatusIndicatorProps,
) -> impl Into<AnyElement<'static>> {
    match props.completed {
        Some(true) => element!(Text(content: CHECKMARK, color: COLOR_COMPLETED)).into_any(),
        Some(false) => element!(Text(content: XMARK, color: COLOR_FAILED)).into_any(),
        None => {
            if props.show_spinner {
                element!(Spinner(color: COLOR_ACTIVE)).into_any()
            } else {
                element!(Text(content: " ")).into_any()
            }
        }
    }
}

/// Build logs viewport height for collapsed preview (press 'e' to expand to fullscreen)
pub const LOG_VIEWPORT_COLLAPSED: usize = 10;
/// Viewport height for failed activities (show more context on failure)
pub const LOG_VIEWPORT_FAILED: usize = 20;
/// Reduced viewport height for tasks with showOutput=true (expands to full when selected)
pub const LOG_VIEWPORT_SHOW_OUTPUT: usize = 3;

fn preview_panel_width(terminal_width: u16, indent: usize) -> u32 {
    terminal_width.saturating_sub(indent as u16).max(1) as u32
}

fn truncate_preview_line(text: &str, max_width: usize) -> String {
    if text.chars().count() > max_width {
        let mut s: String = text.chars().take(max_width.saturating_sub(1)).collect();
        s.push('…');
        s
    } else {
        text.to_string()
    }
}

// Broad ANSI matcher covering CSI, OSC, DCS, and single-byte ESC sequences.
// Collapsed previews are plain-text summaries, so strip escape sequences before width math.
static PREVIEW_ANSI_ESCAPE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\x1B(?:\[[0-?]*[ -/]*[@-~]|[PX^_][^\x1B]*\x1B\\|\][^\x07\x1B]*(?:\x07|\x1B\\)|[@-Z\\-_])",
    )
    .expect("valid ANSI escape regex")
});

// Strip remaining control bytes except tab/newline/carriage return.
static PREVIEW_CONTROL_BYTE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[\x00-\x08\x0B-\x1A\x1C-\x1F\x7F]").expect("valid control-byte regex")
});

fn sanitize_preview_text(text: &str) -> String {
    let no_escapes = PREVIEW_ANSI_ESCAPE_REGEX.replace_all(text, "");
    PREVIEW_CONTROL_BYTE_REGEX
        .replace_all(no_escapes.as_ref(), "")
        .to_string()
}

#[derive(Clone)]
struct PreviewStyledCell {
    ch: char,
    pen: avt::Pen,
}

enum PreviewLine {
    Plain(String),
    Styled(Vec<PreviewStyledCell>),
}

fn preview_pen_color(color: avt::Color) -> Color {
    match color {
        avt::Color::Indexed(c) => Color::AnsiValue(c),
        avt::Color::RGB(rgb) => Color::Rgb {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        },
    }
}

fn build_preview_styled_segment(text: String, pen: &avt::Pen) -> AnyElement<'static> {
    let fg = pen.foreground().map(preview_pen_color);
    let bg = pen.background().map(preview_pen_color);

    let text = element! {
        Text(
            content: text,
            color: fg,
            weight: if pen.is_bold() { Weight::Bold } else { Weight::Normal },
            italic: pen.is_italic(),
            decoration: if pen.is_underline() { TextDecoration::Underline } else { TextDecoration::None },
            wrap: TextWrap::NoWrap,
        )
    }
    .into_any();

    if let Some(background_color) = bg {
        element!(View(background_color: background_color) { #(vec![text]) }).into_any()
    } else {
        text
    }
}

fn is_blank_vt_row(row: &[avt::Cell]) -> bool {
    row.iter().all(|cell| cell.char() == ' ')
}

fn trim_trailing_blank_vt_rows<'a>(rows: &'a [&'a [avt::Cell]]) -> &'a [&'a [avt::Cell]] {
    let Some(last_content_idx) = rows.iter().rposition(|row| !is_blank_vt_row(row)) else {
        return &[];
    };

    &rows[..=last_content_idx]
}

/// Format elapsed time for display: ms -> s -> m s -> h m
/// When `high_resolution` is true, shows ms for sub-second durations.
/// When `high_resolution` is false, hides if < 300ms, otherwise shows x.xs resolution.
pub fn format_elapsed_time(elapsed: Duration, high_resolution: bool) -> String {
    if cfg!(feature = "deterministic-tui") {
        return "[TIME]".to_string();
    }
    let total_secs = elapsed.as_secs();
    if total_secs < 1 {
        if high_resolution {
            format!("{}ms", elapsed.as_millis())
        } else if elapsed.as_millis() >= 300 {
            format!("{:.1}s", elapsed.as_secs_f64())
        } else {
            String::new()
        }
    } else if total_secs < 60 {
        format!("{:.1}s", elapsed.as_secs_f64())
    } else if total_secs < 3600 {
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{}m {}s", mins, secs)
    } else {
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        format!("{}h {}m", hours, mins)
    }
}

/// Component for rendering hierarchy structure (indentation + branch) for nested activities.
/// Only used for nested items (depth > 0). Top-level items don't need hierarchy rendering.
pub struct HierarchyPrefixComponent {
    pub depth: usize,
}

impl HierarchyPrefixComponent {
    pub fn new(depth: usize) -> Self {
        Self { depth }
    }

    /// Renders the hierarchy prefix: [indent][branch]
    /// The indent aligns with parent's content (after their status indicator).
    pub fn render(&self) -> Vec<AnyElement<'static>> {
        if self.depth == 0 {
            return vec![];
        }

        // Indentation: 2 spaces for status indicator width + 2 spaces per additional nesting level
        let status_indicator_offset = 2;
        let nesting_indent = "  ".repeat(self.depth - 1);
        let total_indent = format!("{}{}", " ".repeat(status_indicator_offset), nesting_indent);

        vec![
            element!(Text(content: total_indent)).into_any(),
            element!(View(margin_right: 1) {
                Text(content: "└", color: COLOR_HIERARCHY)
            })
            .into_any(),
        ]
    }
}

/// Component for rendering colored activity text
pub struct ActivityTextComponent {
    pub action: String,
    pub name: String,
    pub suffix: Option<String>,
    pub is_selected: bool,
    pub elapsed: String,
    pub is_completed: bool,
    pub variant: ActivityVariant,
}

impl ActivityTextComponent {
    pub fn new(action: String, name: String, elapsed: String, variant: ActivityVariant) -> Self {
        Self {
            action,
            name,
            suffix: None,
            is_selected: false,
            elapsed,
            is_completed: false,
            variant,
        }
    }

    /// Create a component that displays only the name (no action prefix).
    /// Use this for activities where the name is self-describing (e.g., "Evaluating Nix").
    pub fn name_only(name: String, elapsed: String, variant: ActivityVariant) -> Self {
        Self::new(String::new(), name, elapsed, variant)
    }

    pub fn with_suffix(mut self, suffix: Option<String>) -> Self {
        self.suffix = suffix;
        self
    }

    pub fn with_selection(mut self, is_selected: bool) -> Self {
        self.is_selected = is_selected;
        self
    }

    pub fn with_completed(mut self, completed: bool) -> Self {
        self.is_completed = completed;
        self
    }

    pub fn render(
        &self,
        terminal_width: u16,
        depth: usize,
        prefix_children: Vec<AnyElement<'static>>,
    ) -> AnyElement<'static> {
        let (shortened_name, display_suffix) = calculate_display_info(
            &self.name,
            terminal_width as u32,
            &self.action,
            self.suffix.as_deref(),
            &self.elapsed,
            depth,
        );

        // Colors: blue when active, green when completed
        // Selected rows get inverted colors
        let (name_color, suffix_color, elapsed_color, bg_color) = if self.is_selected {
            (
                Color::AnsiValue(232),       // Near-black text
                Color::AnsiValue(238),       // Dark gray for suffix
                Color::AnsiValue(238),       // Dark gray for elapsed
                Some(Color::AnsiValue(250)), // Light gray background
            )
        } else if self.is_completed && depth == 0 {
            (Color::Reset, COLOR_SECONDARY, COLOR_HIERARCHY, None)
        } else if self.is_completed {
            (COLOR_ACTIVE_NESTED, COLOR_SECONDARY, COLOR_HIERARCHY, None)
        } else if depth == 0 || matches!(self.variant, ActivityVariant::Process(_)) {
            (COLOR_ACTIVE, COLOR_SECONDARY, COLOR_HIERARCHY, None)
        } else {
            (COLOR_ACTIVE_NESTED, COLOR_SECONDARY, COLOR_HIERARCHY, None)
        };

        let mut final_prefix = prefix_children;

        // Only add action text if action is not empty
        if !self.action.is_empty() {
            // Action word should be capitalized
            let action_text = {
                let mut chars = self.action.chars();
                match chars.next() {
                    Some(first) => {
                        format!(
                            "{}{}",
                            first.to_uppercase().collect::<String>(),
                            chars.as_str()
                        )
                    }
                    None => String::new(),
                }
            };
            final_prefix.push(
                element!(View(width: (action_text.chars().count() + 1) as u32, flex_shrink: 0.0) {
                    View(margin_right: 1) {
                        Text(content: action_text, color: name_color, weight: Weight::Bold)
                    }
                })
                .into_any(),
            );
        }

        if let Some(bg) = bg_color {
            element! {
                View(height: 1, width: 100pct, flex_direction: FlexDirection::Row, padding_right: 1, background_color: bg) {
                    // Fixed left column - never truncates
                    View(flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                        #(final_prefix)
                    }
                    // Flexible middle column - can overflow
                    // Each item uses leading margin (margin_left) to separate from predecessor
                    View(flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden, margin_right: 1, flex_direction: FlexDirection::Row) {
                        #(if !shortened_name.is_empty() {
                            let has_predecessor = !self.action.is_empty();
                            let margin = if has_predecessor { 1 } else { 0 };
                            vec![element!(View(margin_left: margin) {
                                Text(content: shortened_name, color: name_color, weight: Weight::Bold)
                            }).into_any()]
                        } else {
                            vec![]
                        })
                        #(if let Some(ref suffix_text) = display_suffix {
                            // Suffix always has a predecessor (action or name)
                            vec![element!(View(margin_left: 1) {
                                Text(content: suffix_text, color: suffix_color)
                            }).into_any()]
                        } else {
                            vec![]
                        })
                    }
                    // Fixed right column - never truncates
                    View(flex_shrink: 0.0) {
                        Text(content: self.elapsed.clone(), color: elapsed_color)
                    }
                }
            }
            .into()
        } else {
            element! {
                View(height: 1, width: 100pct, flex_direction: FlexDirection::Row, padding_right: 1) {
                    // Fixed left column - never truncates
                    View(flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                        #(final_prefix)
                    }
                    // Flexible middle column - can overflow
                    // Each item uses leading margin (margin_left) to separate from predecessor
                    View(flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden, margin_right: 1, flex_direction: FlexDirection::Row) {
                        #(if !shortened_name.is_empty() {
                            let has_predecessor = !self.action.is_empty();
                            let margin = if has_predecessor { 1 } else { 0 };
                            vec![element!(View(margin_left: margin) {
                                Text(content: shortened_name, color: name_color, weight: Weight::Bold)
                            }).into_any()]
                        } else {
                            vec![]
                        })
                        #(if let Some(ref suffix_text) = display_suffix {
                            // Suffix always has a predecessor (action or name)
                            vec![element!(View(margin_left: 1) {
                                Text(content: suffix_text, color: suffix_color)
                            }).into_any()]
                        } else {
                            vec![]
                        })
                    }
                    // Fixed right column - never truncates
                    View(flex_shrink: 0.0) {
                        Text(content: self.elapsed.clone(), color: elapsed_color)
                    }
                }
            }
            .into()
        }
    }
}

/// Component for rendering download progress bars
pub struct ProgressBarComponent {
    pub percent: u8,
    pub downloaded_text: String,
    pub total_text: String,
    pub speed_text: Option<String>,
    pub indent: String,
}

impl ProgressBarComponent {
    pub fn new(percent: u8, downloaded_text: String, total_text: String, indent: String) -> Self {
        Self {
            percent,
            downloaded_text,
            total_text,
            speed_text: None,
            indent,
        }
    }

    pub fn with_speed(mut self, speed_text: String) -> Self {
        self.speed_text = Some(speed_text);
        self
    }

    pub fn render(&self, terminal_width: u16) -> AnyElement<'static> {
        // Progress bar indented more than parent
        let progress_indent = format!("{}    ", self.indent);

        // Calculate space for progress bar - leave room for size info and speed
        let size_info = if let Some(ref speed) = self.speed_text {
            format!(
                "{} / {} at {}",
                self.downloaded_text, self.total_text, speed
            )
        } else {
            format!("{} / {}", self.downloaded_text, self.total_text)
        };

        let prefix_len = progress_indent.len();
        let size_info_len = size_info.len() + 2; // +2 for spaces

        // Calculate available width for progress bar
        let available_width = (terminal_width as usize)
            .saturating_sub(prefix_len)
            .saturating_sub(size_info_len)
            .saturating_sub(4); // Some padding
        let bar_width = available_width.clamp(10, 100); // Min 10, max 100 chars

        let filled = (bar_width * self.percent as usize) / 100;
        let empty = bar_width - filled;

        // Split progress bar into filled and empty parts for coloring
        let filled_bar = "─".repeat(filled);
        let empty_bar = "─".repeat(empty);

        element! {
            View(height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, width: 100pct) {
                View(flex_direction: FlexDirection::Row) {
                    Text(content: progress_indent)
                    Text(content: filled_bar, color: COLOR_ACTIVE)
                    Text(content: empty_bar, color: Color::AnsiValue(238))
                }
                Text(content: size_info, color: COLOR_HIERARCHY)
            }
        }
        .into_any()
    }
}

/// Component for rendering download activities with progress
pub struct DownloadActivityComponent<'a> {
    pub activity: &'a Activity,
    pub depth: usize,
    pub is_selected: bool,
    /// Completion state: None = active, Some(true) = success, Some(false) = failed
    pub completed: Option<bool>,
    /// Whether this activity's result was cached
    pub cached: bool,
}

impl<'a> DownloadActivityComponent<'a> {
    pub fn new(activity: &'a Activity, depth: usize, is_selected: bool) -> Self {
        Self {
            activity,
            depth,
            is_selected,
            completed: None,
            cached: false,
        }
    }

    pub fn with_completed(mut self, completed: Option<bool>) -> Self {
        self.completed = completed;
        self
    }

    pub fn with_cached(mut self, cached: bool) -> Self {
        self.cached = cached;
        self
    }

    pub fn render(&self, terminal_width: u16) -> AnyElement<'static> {
        let indent = "  ".repeat(self.depth);
        // Use stored duration for completed activities, skip for queued
        let elapsed_str = match &self.activity.state {
            NixActivityState::Completed { duration, .. } => format_elapsed_time(*duration, true),
            NixActivityState::Active => {
                format_elapsed_time(self.activity.start_time.elapsed(), false)
            }
            NixActivityState::Queued => String::new(),
        };

        let mut elements = vec![];

        // First line: activity name with hierarchy prefix and status indicator
        let mut prefix = HierarchyPrefixComponent::new(self.depth).render();
        prefix.push(
            element!(View(margin_right: 1) {
                StatusIndicator(completed: self.completed, show_spinner: true)
            })
            .into_any(),
        );

        // Get substituter from download variant
        let substituter =
            if let ActivityVariant::Download(ref download_data) = self.activity.variant {
                download_data.substituter.as_ref()
            } else {
                None
            };

        let (shortened_name, _) = calculate_display_info(
            &self.activity.short_name,
            terminal_width as u32,
            "Downloading",
            substituter.map(|s| format!("from {}", s)).as_deref(),
            &elapsed_str,
            self.depth,
        );

        // Colors for selected vs unselected rows - invert all text when selected
        let (action_color, name_color, substituter_color, elapsed_color, bg_color) =
            if self.is_selected {
                (
                    COLOR_ACTIVE,
                    Color::AnsiValue(232),       // Near-black text
                    Color::AnsiValue(238),       // Dark gray for substituter
                    Color::AnsiValue(238),       // Dark gray for elapsed
                    Some(Color::AnsiValue(250)), // Light gray background
                )
            } else {
                (
                    COLOR_ACTIVE_NESTED,
                    Color::Reset,
                    COLOR_SECONDARY,
                    COLOR_HIERARCHY,
                    None,
                )
            };

        let mut line1_children = prefix;
        line1_children.extend(vec![
            element!(View(margin_right: 1) {
                Text(content: "Downloading", color: action_color, weight: Weight::Bold)
            })
            .into_any(),
            element!(View(margin_right: 1) {
                Text(content: shortened_name, color: name_color)
            })
            .into_any(),
        ]);

        if let Some(substituter) = &substituter {
            // Only show "from" text on wider terminals
            if terminal_width >= 80 {
                line1_children.push(
                    element!(Text(content: format!("from {}", substituter), color: substituter_color))
                        .into_any(),
                );
            }
        }

        if let Some(bg) = bg_color {
            elements.push(
                element! {
                    View(height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, width: 100pct, padding_right: 1, overflow: Overflow::Hidden, background_color: bg) {
                        View(flex_direction: FlexDirection::Row, width: 100pct, overflow: Overflow::Hidden) {
                            #(line1_children)
                        }
                        View {
                            Text(content: elapsed_str.clone(), color: elapsed_color)
                        }
                    }
                }
                .into_any()
            );
        } else {
            elements.push(
                element! {
                    View(height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, width: 100pct, padding_right: 1, overflow: Overflow::Hidden) {
                        View(flex_direction: FlexDirection::Row, width: 100pct, overflow: Overflow::Hidden) {
                            #(line1_children)
                        }
                        View {
                            Text(content: elapsed_str.clone(), color: elapsed_color)
                        }
                    }
                }
                .into_any()
            );
        }

        // Second line: progress bar if we have progress data
        if let ActivityVariant::Download(ref download_data) = self.activity.variant {
            if let (Some(downloaded), Some(total)) =
                (download_data.size_current, download_data.size_total)
            {
                let percent = (downloaded as f64 / total as f64 * 100.0) as u8;
                let human_downloaded = downloaded.human_count_bytes().to_string();
                let human_total = total.human_count_bytes().to_string();
                let speed = download_data
                    .speed
                    .unwrap_or(0)
                    .human_throughput_bytes()
                    .to_string();

                let progress_bar =
                    ProgressBarComponent::new(percent, human_downloaded, human_total, indent)
                        .with_speed(speed);
                elements.push(progress_bar.render(terminal_width));
            } else if let Some(progress) = &self.activity.progress
                && progress.total.unwrap_or(0) > 0
            {
                let current = progress.current.unwrap_or(0);
                let total = progress.total.unwrap_or(1);
                let percent = (current as f64 / total as f64 * 100.0) as u8;
                let human_done = current.human_count_bytes().to_string();
                let human_expected = total.human_count_bytes().to_string();

                let progress_bar =
                    ProgressBarComponent::new(percent, human_done, human_expected, indent);
                elements.push(progress_bar.render(terminal_width));
            }
        }

        element! {
            View(flex_direction: FlexDirection::Column) {
                #(elements)
            }
        }
        .into_any()
    }
}

/// Calculate display info for activity considering terminal width.
///
/// Returns `(shortened_name, optional_shortened_suffix)`. When space is tight,
/// the suffix is truncated from the right first, then dropped, then the name
/// is truncated from the left.
pub fn calculate_display_info(
    path: &str,
    terminal_width: u32,
    action: &str,
    suffix: Option<&str>,
    elapsed: &str,
    depth: usize,
) -> (String, Option<String>) {
    // Calculate base width: padding + indent + hierarchy + spinner + action + name_margin + elapsed
    let indent_width = if depth > 0 {
        2 + (depth - 1) * 2 // spinner offset (2) + nesting indent
    } else {
        0
    };
    let hierarchy_width = if depth > 0 { 2 } else { 0 }; // "⎿" + margin_right: 1 for indented items
    let action_width = action.len() + 1; // action + margin_right
    let name_margin_width = 1; // margin_right after name
    let elapsed_width = elapsed.len();
    let padding_width = 2; // left + right padding
    let spinner_width = if depth == 0 { 2 } else { 0 }; // "⠋ " for top-level items

    let base_width = padding_width
        + indent_width
        + hierarchy_width
        + spinner_width
        + action_width
        + name_margin_width
        + elapsed_width;
    let available_width = terminal_width as usize;

    if base_width >= available_width {
        // Very constrained, hide suffix and use shortest possible path
        return (shorten_store_path_aggressive(path), None);
    }

    let remaining = available_width - base_width;
    let suffix_total = suffix.map(|s| s.chars().count() + 1).unwrap_or(0); // +1 for leading margin

    // Everything fits
    if path.len() + suffix_total <= remaining {
        return (path.to_string(), suffix.map(|s| s.to_string()));
    }

    // Doesn't fit. Truncate suffix first, then drop it, then truncate name.
    if let Some(suffix_str) = suffix {
        let suffix_chars: Vec<char> = suffix_str.chars().collect();
        // How much space is left for suffix after the name?
        let space_for_suffix = remaining.saturating_sub(path.len() + 1); // +1 for margin
        if space_for_suffix >= suffix_chars.len() {
            // Suffix fits, name is the problem
            return (path.to_string(), Some(suffix_str.to_string()));
        }
        if space_for_suffix >= 2 {
            // Truncate suffix from the right
            let kept: String = suffix_chars[..space_for_suffix - 1].iter().collect();
            return (path.to_string(), Some(format!("{}…", kept)));
        }
        // No room for suffix at all, drop it
        if path.len() <= remaining {
            return (path.to_string(), None);
        }
    }

    // No suffix (or dropped). Truncate name from the left.
    if remaining > 4 {
        let chars: Vec<char> = path.chars().collect();
        let start_char = chars.len().saturating_sub(remaining - 1);
        let truncated_chars: String = chars.iter().skip(start_char).collect();
        return (format!("…{}", truncated_chars), None);
    }

    // Extremely narrow
    ("…".to_string(), None)
}

/// Aggressively shorten a store path for very narrow terminals
fn shorten_store_path_aggressive(path: &str) -> String {
    if let Some(store_start) = path.find("/nix/store/") {
        let before_store = &path[..store_start];
        let after_store = &path[store_start + 11..]; // Skip "/nix/store/"

        if let Some(dash_pos) = after_store.find('-') {
            let rest = &after_store[dash_pos..];
            // Use ellipsis for hash but keep the package name
            return format!("{}/nix/store/…{}", before_store, rest);
        }
    }

    // Check if this looks like a bare hash-packagename (no /nix/store/ prefix)
    if let Some(dash_pos) = path.find('-') {
        let before_dash = &path[..dash_pos];
        let after_dash = &path[dash_pos + 1..]; // Skip the dash

        // If the part before dash looks like a hash (long alphanumeric), just show package name
        if before_dash.len() > 10 && before_dash.chars().all(|c| c.is_alphanumeric()) {
            return after_dash.to_string();
        }
    }

    // Fallback: if it still looks like a hash, truncate and add ellipsis
    if path.len() > 15 && path.chars().all(|c| c.is_alphanumeric()) {
        // Looks like just a hash, truncate to first few chars + ellipsis
        format!("{}…", &path[..4])
    } else if path.len() > 20 {
        // For file paths (like evaluation paths), keep the end and truncate the beginning
        if path.contains('/') {
            format!("…{}", &path[path.len() - 19..])
        } else {
            format!("{}…", &path[..19])
        }
    } else {
        path.to_string()
    }
}

/// Component for rendering collapsed content preview (logs, details, traces) inline below activities.
/// Press 'e' to expand to fullscreen view with scrolling.
pub struct ExpandedContentComponent<'a> {
    pub lines: Option<&'a VecDeque<String>>,
    pub vt: Option<&'a Arc<std::sync::Mutex<avt::Vt>>>,
    pub empty_message: &'a str,
    pub max_lines: usize,
    pub depth: usize,
}

impl<'a> ExpandedContentComponent<'a> {
    pub fn new(lines: Option<&'a VecDeque<String>>) -> Self {
        Self {
            lines,
            vt: None,
            empty_message: "  → no content",
            max_lines: LOG_VIEWPORT_COLLAPSED,
            depth: 0,
        }
    }

    pub fn with_vt(mut self, vt: Option<&'a Arc<std::sync::Mutex<avt::Vt>>>) -> Self {
        self.vt = vt;
        self
    }

    pub fn with_max_lines(mut self, max_lines: usize) -> Self {
        self.max_lines = max_lines;
        self
    }

    pub fn with_empty_message(mut self, message: &'a str) -> Self {
        self.empty_message = message;
        self
    }

    pub fn with_depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    pub fn render(&self, terminal_width: u16) -> Vec<AnyElement<'static>> {
        let preview_lines = self.preview_lines();
        if preview_lines.is_empty() {
            return vec![self.render_preview_block(
                vec![PreviewLine::Plain(self.empty_message.to_string())],
                terminal_width,
            )];
        }

        vec![self.render_preview_block(preview_lines, terminal_width)]
    }

    /// Calculate the height this component will take
    pub fn calculate_height(&self) -> usize {
        if let Some(vt) = self.vt {
            let vt = vt.lock().unwrap();
            let rows: Vec<&[avt::Cell]> = vt.lines().map(|row| row.cells()).collect();
            let count = trim_trailing_blank_vt_rows(&rows).len();
            if count > 0 {
                return count.min(self.max_lines);
            }
        }
        if let Some(lines) = &self.lines
            && !lines.is_empty()
        {
            let visible_count = lines.len().min(self.max_lines);
            if visible_count > 0 {
                return visible_count;
            }
        }
        1 // Minimal height for empty message
    }

    /// Render the component with a main activity line, returning a single element
    /// with proper height calculation for the combined content.
    pub fn render_with_main_line(
        &self,
        main_line: AnyElement<'static>,
        terminal_width: u16,
    ) -> AnyElement<'static> {
        let mut elements = vec![main_line];
        elements.extend(self.render(terminal_width));

        let total_height = (1 + self.calculate_height()) as u32;

        element! {
            View(height: total_height, width: 100pct, flex_direction: FlexDirection::Column) {
                #(elements)
            }
        }
        .into_any()
    }

    fn preview_lines(&self) -> Vec<PreviewLine> {
        if let Some(vt) = self.vt {
            let vt = vt.lock().unwrap();
            let rows: Vec<&[avt::Cell]> = vt.lines().map(|row| row.cells()).collect();
            let rows = trim_trailing_blank_vt_rows(&rows);
            let mut lines = Vec::new();
            for row in rows.iter().rev().take(self.max_lines).rev() {
                lines.push(PreviewLine::Styled(
                    row.iter()
                        .map(|cell| PreviewStyledCell {
                            ch: cell.char(),
                            pen: cell.pen().clone(),
                        })
                        .collect(),
                ));
            }
            if !lines.is_empty() {
                return lines;
            }
        }

        if let Some(lines) = &self.lines {
            return lines
                .iter()
                .rev()
                .take(self.max_lines)
                .rev()
                .cloned()
                .map(PreviewLine::Plain)
                .collect();
        }

        Vec::new()
    }

    fn render_preview_block(
        &self,
        lines: Vec<PreviewLine>,
        terminal_width: u16,
    ) -> AnyElement<'static> {
        let indent = 2 + (self.depth * 2);
        let panel_width = preview_panel_width(terminal_width, indent);
        let text_width = (panel_width as usize).saturating_sub(2);

        let rows: Vec<_> = lines
            .into_iter()
            .map(|line| {
                let content = match line {
                    PreviewLine::Plain(line) => {
                        let line = sanitize_preview_text(&line);
                        let display_text = truncate_preview_line(&line, text_width);
                        element! {
                            View(
                                width: text_width as u32,
                                flex_shrink: 0.0,
                                flex_direction: FlexDirection::Row,
                                overflow: Overflow::Hidden,
                            ) {
                                View(flex_shrink: 0.0, overflow: Overflow::Hidden) {
                                    Text(content: display_text, color: Color::AnsiValue(245), wrap: TextWrap::NoWrap)
                                }
                            }
                        }
                        .into_any()
                    }
                    PreviewLine::Styled(cells) => {
                        self.build_styled_preview_content(cells, text_width)
                    }
                };
                element! {
                    View(
                        height: 1,
                        width: 100pct,
                        flex_direction: FlexDirection::Row,
                        overflow: Overflow::Hidden,
                    ) {
                        View(
                            width: 1,
                            flex_shrink: 0.0,
                        ) {
                            Text(content: "│", color: Color::AnsiValue(245), wrap: TextWrap::NoWrap)
                        }
                        View(
                            width: 1,
                            flex_shrink: 0.0,
                        ) {}
                        #(vec![content])
                    }
                }
                .into_any()
            })
            .collect();

        let row_count = rows.len() as u32;

        element! {
            View(height: row_count, width: 100pct, flex_direction: FlexDirection::Column, overflow: Overflow::Hidden) {
                View(width: 100pct, padding_left: indent as u32, flex_direction: FlexDirection::Column, overflow: Overflow::Hidden) {
                    #(rows)
                }
            }
        }
        .into_any()
    }

    fn build_styled_preview_content(
        &self,
        cells: Vec<PreviewStyledCell>,
        text_width: usize,
    ) -> AnyElement<'static> {
        let mut clipped = cells;
        if clipped.len() > text_width {
            clipped.truncate(text_width);
            if let Some(last) = clipped.last_mut() {
                last.ch = '…';
            }
        }
        while clipped.len() < text_width {
            clipped.push(PreviewStyledCell {
                ch: ' ',
                pen: avt::Pen::default(),
            });
        }

        let mut runs: Vec<(String, avt::Pen)> = Vec::new();
        for cell in clipped {
            if let Some((text, pen)) = runs.last_mut()
                && *pen == cell.pen
            {
                text.push(cell.ch);
            } else {
                runs.push((cell.ch.to_string(), cell.pen));
            }
        }

        let run_elements: Vec<_> = runs
            .into_iter()
            .map(|(text, pen)| build_preview_styled_segment(text, &pen))
            .collect();

        element! {
            View(
                width: text_width as u32,
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Row,
                overflow: Overflow::Hidden,
            ) {
                #(run_elements)
            }
        }
        .into_any()
    }
}

/// Backwards-compatible alias
pub type BuildLogsComponent<'a> = ExpandedContentComponent<'a>;

#[cfg(test)]
mod tests {
    use super::*;

    // For depth=0, action="", elapsed="1.0s":
    // base_width = padding(2) + spinner(2) + action(0+1) + name_margin(1) + elapsed(4) = 10
    // remaining = terminal_width - 10

    #[test]
    fn everything_fits_with_suffix() {
        // remaining = 100 - 10 = 90, name(24) + suffix(7+1margin) = 32 fits
        let (name, suffix) = calculate_display_info(
            "devenv:python:virtualenv",
            100,
            "",
            Some("4 lines"),
            "1.0s",
            0,
        );
        assert_eq!(name, "devenv:python:virtualenv");
        assert_eq!(suffix.as_deref(), Some("4 lines"));
    }

    #[test]
    fn everything_fits_without_suffix() {
        let (name, suffix) =
            calculate_display_info("devenv:python:virtualenv", 80, "", None, "1.0s", 0);
        assert_eq!(name, "devenv:python:virtualenv");
        assert_eq!(suffix, None);
    }

    #[test]
    fn long_suffix_truncated_before_name() {
        // remaining = 60 - 10 = 50, name=24
        // space_for_suffix = 50 - 24 - 1(margin) = 25 chars
        // suffix(63 chars) > 25 so truncated to 25 chars (24 kept + …)
        let long_suffix = "4 lines → DEVENV_EXPORT:VklSVFVBTF9FTlY==L2hvbWUvZG9tZW4vZGV2";
        let (name, suffix) = calculate_display_info(
            "devenv:python:virtualenv",
            60,
            "",
            Some(long_suffix),
            "1.0s",
            0,
        );
        assert_eq!(name, "devenv:python:virtualenv");
        let suffix = suffix.expect("suffix should be shown");
        assert!(
            suffix.starts_with("4 lines"),
            "truncation preserves the start"
        );
        assert!(suffix.ends_with('…'));
        assert_eq!(suffix.chars().count(), 25);
    }

    #[test]
    fn suffix_dropped_when_only_1_char_available() {
        // remaining = 36 - 10 = 26, name=24
        // space_for_suffix = 26 - 24 - 1(margin) = 1 char, which is < 2 so suffix is dropped
        let (name, suffix) = calculate_display_info(
            "devenv:python:virtualenv",
            36,
            "",
            Some("cached"),
            "1.0s",
            0,
        );
        assert_eq!(name, "devenv:python:virtualenv");
        assert_eq!(suffix, None);
    }

    #[test]
    fn name_truncated_left_when_no_suffix() {
        // remaining = 20 - 10 = 10, name=24 doesn't fit, 10 > 4 so left-truncate
        // start_char = 24 - (10-1) = 15, keeps 9 chars + "…" = 10 chars
        let (name, suffix) =
            calculate_display_info("devenv:python:virtualenv", 20, "", None, "1.0s", 0);
        assert!(name.starts_with('…'));
        assert_eq!(name.chars().count(), 10);
        assert_eq!(suffix, None);
    }

    #[test]
    fn name_truncated_after_suffix_dropped() {
        // remaining = 20 - 10 = 10, name=24 doesn't fit
        // suffix can't fit either, gets dropped, then name is left-truncated to 10 chars
        let (name, suffix) = calculate_display_info(
            "devenv:python:virtualenv",
            20,
            "",
            Some("4 lines"),
            "1.0s",
            0,
        );
        assert!(name.starts_with('…'));
        assert_eq!(name.chars().count(), 10);
        assert_eq!(suffix, None);
    }

    #[test]
    fn very_constrained_uses_aggressive_shortening() {
        // base=10 >= terminal=5, hits shorten_store_path_aggressive
        let (name, suffix) = calculate_display_info(
            "/nix/store/abc123hash-some-package-1.0",
            5,
            "",
            Some("4 lines"),
            "1.0s",
            0,
        );
        assert_eq!(suffix, None);
        // shorten_store_path_aggressive keeps package name after hash
        assert!(name.contains("some-package"), "got: {}", name);
    }

    #[test]
    fn extremely_narrow_remaining() {
        // remaining = 14 - 10 = 4, which is <= 4 so just "…"
        let (name, suffix) =
            calculate_display_info("devenv:python:virtualenv", 14, "", None, "1.0s", 0);
        assert_eq!(name, "…");
        assert_eq!(suffix, None);
    }

    #[test]
    fn nesting_reduces_budget() {
        // depth=0: base = 10, remaining = 42-10 = 32
        // depth=2: base = padding(2)+indent(4)+hierarchy(2)+spinner(0)+action(0+1)+name_margin(1)+elapsed(4) = 14
        //          remaining = 42 - 14 = 28
        // name(24) + suffix("cached" 6+1margin) = 31
        // depth=0: 32 >= 31, fits
        // depth=2: 28 < 31, doesn't fit, space_for_suffix = 28-24-1 = 3 >= 2, so suffix truncated
        let (_, suffix_shallow) = calculate_display_info(
            "devenv:python:virtualenv",
            42,
            "",
            Some("cached"),
            "1.0s",
            0,
        );
        let (name_deep, suffix_deep) = calculate_display_info(
            "devenv:python:virtualenv",
            42,
            "",
            Some("cached"),
            "1.0s",
            2,
        );
        assert_eq!(suffix_shallow.as_deref(), Some("cached"));
        assert_eq!(name_deep, "devenv:python:virtualenv");
        let s = suffix_deep.expect("suffix should be truncated not dropped");
        assert!(s.ends_with('…'));
        assert_eq!(s.chars().count(), 3);
    }

    #[test]
    fn action_reduces_budget() {
        // action="building"(8+1=9)
        // base = padding(2)+spinner(2)+action(9)+name_margin(1)+elapsed(4) = 18
        // remaining = 60 - 18 = 42, name("some-package")=12
        // space_for_suffix = 42 - 12 - 1 = 29
        // suffix(56 chars) > 29 so truncated to 29 chars (28 kept + …)
        let long_suffix = "4 lines → DEVENV_EXPORT:VklSVFVBTF9FTlY==L2hvbWUvZG9tZW4";
        let (name, suffix) =
            calculate_display_info("some-package", 60, "building", Some(long_suffix), "1.0s", 0);
        assert_eq!(name, "some-package");
        let suffix = suffix.expect("suffix should be truncated");
        assert!(suffix.ends_with('…'));
        assert_eq!(suffix.chars().count(), 29);
    }

    #[test]
    fn multibyte_suffix_does_not_panic() {
        // Suffix with multi-byte UTF-8 chars: .len() (bytes) > .chars().count()
        // This previously panicked because byte length was used for comparison
        // but char index was used for slicing.
        // "ä" is 2 bytes in UTF-8, so 30 chars = 60 bytes.
        // With terminal_width=80, action="building", path="pkg":
        //   base_width = 2+2+9+1+4 = 18, remaining = 62
        //   space_for_suffix = 62 - 3 - 1 = 58
        //   Old code: 58 >= suffix.len()(60) → false, then chars[..57] on 30-char vec → panic!
        let suffix = "ääääääääääääääääääääääääääääää"; // 30 chars, 60 bytes
        let (name, _suffix) =
            calculate_display_info("pkg", 80, "building", Some(suffix), "1.0s", 0);
        assert_eq!(name, "pkg");
    }

    #[test]
    fn expanded_preview_lines_do_not_render_trailing_border() {
        let lines = VecDeque::from(["short".to_string(), "tiny".to_string()]);
        let component = ExpandedContentComponent::new(Some(&lines));
        let mut elements = component.render(20);
        let output = elements.remove(0).render(Some(20)).to_string();

        for line in output.lines() {
            assert_ne!(
                line.chars().last(),
                Some('│'),
                "line should not render a trailing preview border: {line:?}"
            );
        }
    }

    #[test]
    fn nested_expanded_preview_does_not_render_trailing_border() {
        let lines = VecDeque::from(["nested".to_string()]);
        let component = ExpandedContentComponent::new(Some(&lines)).with_depth(2);
        let mut elements = component.render(24);
        let output = elements.remove(0).render(Some(24)).to_string();

        for line in output.lines() {
            assert_ne!(
                line.chars().last(),
                Some('│'),
                "line should not render a trailing preview border: {line:?}"
            );
        }
    }

    #[test]
    fn sanitize_preview_text_removes_terminal_sequences() {
        let input = "\x1b[32mgreen\x1b[0m \x1b]8;;https://example.com\x07link\x1b]8;;\x07\x07";
        assert_eq!(sanitize_preview_text(input), "green link");
    }

    #[test]
    fn sanitize_preview_text_removes_non_printable_controls() {
        let input = "ab\x00\x1f\x7fcd\t";
        assert_eq!(sanitize_preview_text(input), "abcd\t");
    }

    #[test]
    fn preview_render_strips_ansi_before_layout() {
        let lines = VecDeque::from(["\x1b[32mgreen\x1b[0m tail".to_string()]);
        let component = ExpandedContentComponent::new(Some(&lines));
        let mut elements = component.render(20);
        let output = elements.remove(0).render(Some(20)).to_string();

        assert!(output.contains("green tail"));
        assert!(!output.contains("\x1b"));
    }

    #[test]
    fn vt_preview_preserves_styled_text_layout() {
        let vt = Arc::new(std::sync::Mutex::new(avt::Vt::new(20, 4)));
        {
            let mut vt = vt.lock().unwrap();
            vt.feed_str("\x1b[32mgreen\x1b[0m tail\n");
        }

        let component = ExpandedContentComponent::new(None).with_vt(Some(&vt));
        let mut elements = component.render(20);
        let output = elements.remove(0).render(Some(20)).to_string();

        assert!(output.contains("green tail"));
        for line in output.lines() {
            assert_ne!(line.chars().last(), Some('│'));
        }
    }
}
