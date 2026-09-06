//! Reusable UI components for the TUI

use crate::model::{Activity, ActivityVariant, NixActivityState};
use devenv_activity::ProcessStatus;
use human_repr::{HumanCount, HumanThroughput};
use iocraft::prelude::*;
use std::collections::VecDeque;
#[cfg(not(feature = "deterministic-tui"))]
use std::sync::Arc;
use std::time::Duration;
#[cfg(not(feature = "deterministic-tui"))]
use tokio::sync::Notify;

// Import shared UI constants from devenv-shell
pub use devenv_shell::{
    CHECKMARK, COLOR_ACTIVE, COLOR_ACTIVE_NESTED, COLOR_COMPLETED, COLOR_FAILED, COLOR_HIERARCHY,
    COLOR_INFO, COLOR_INTERACTIVE, COLOR_SECONDARY, COLOR_TRANSIENT, DOT_HALF, DOT_INERT,
    DOT_READY, DOT_RING, DOT_RUNNING, PULSE_INTERVAL_MS, SPINNER_FRAMES, SPINNER_INTERVAL_MS,
    XMARK,
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

/// Map a process status to its status-dot glyph, color, and whether it pulses.
///
/// Shape carries the lifecycle so the state reads without relying on color
/// (color only reinforces); `pulse` marks transient states so motion signals
/// "in progress" without an animated spinner.
pub fn process_status_dot(
    status: &ProcessStatus,
    completed: Option<bool>,
    shutting_down: bool,
) -> (&'static str, Color, bool) {
    // Global shutdown: every still-active process is draining.
    if shutting_down && status.is_active() {
        return (DOT_HALF, COLOR_HIERARCHY, true);
    }
    match status {
        ProcessStatus::NotStarted => (DOT_INERT, COLOR_HIERARCHY, false),
        ProcessStatus::Waiting => (DOT_RING, COLOR_TRANSIENT, true),
        ProcessStatus::Starting | ProcessStatus::Restarting => (DOT_HALF, COLOR_TRANSIENT, true),
        ProcessStatus::Running => (DOT_RUNNING, COLOR_COMPLETED, false),
        ProcessStatus::Ready => (DOT_READY, COLOR_COMPLETED, false),
        ProcessStatus::Stopping => (DOT_HALF, COLOR_HIERARCHY, true),
        ProcessStatus::Stopped if completed == Some(false) => (XMARK, COLOR_FAILED, false),
        ProcessStatus::Stopped => (DOT_RING, COLOR_HIERARCHY, false),
        ProcessStatus::Exited if completed == Some(false) => (XMARK, COLOR_FAILED, false),
        ProcessStatus::Exited => (DOT_RING, COLOR_HIERARCHY, false),
        ProcessStatus::GaveUp => (XMARK, COLOR_FAILED, false),
    }
}

/// Process status dot. Static glyph for stable states; transient states
/// (`pulse = true`) breathe between `color` and gray to signal liveness
/// without the busy churn of a spinner.
#[derive(Default, Props)]
pub struct StatusDotProps {
    pub glyph: String,
    pub color: Option<Color>,
    pub pulse: bool,
}

#[cfg(feature = "deterministic-tui")]
#[component]
pub fn StatusDot(_hooks: Hooks, props: &StatusDotProps) -> impl Into<AnyElement<'static>> {
    let color = props.color.unwrap_or(COLOR_ACTIVE);
    element! {
        Text(content: props.glyph.clone(), color: color)
    }
}

#[cfg(not(feature = "deterministic-tui"))]
#[component]
pub fn StatusDot(mut hooks: Hooks, props: &StatusDotProps) -> impl Into<AnyElement<'static>> {
    let color = props.color.unwrap_or(COLOR_ACTIVE);

    // Hooks must be called unconditionally and in a stable order every render
    // (iocraft rules of hooks); `pulse` can flip as a process changes state,
    // so always register them and gate only the rendering.
    let mut bright = hooks.use_state(|| true);
    // Non-reactive mirror of `pulse`, refreshed every render. The animation
    // future reads it to decide whether to toggle `bright`. Writing a `Ref`
    // does not mark the component dirty, so updating this never forces a redraw.
    let mut pulse_active = hooks.use_ref(|| props.pulse);
    pulse_active.set(props.pulse);
    // Wake source so the animation future can fully park while the dot is
    // steady, instead of polling a timer twice a second per dot — which would
    // scale idle wakeups with the number of processes (#2915). We signal it on
    // the steady -> pulsing transition; `notify_one` stores a permit, so a wake
    // raised before the future parks is never lost.
    let wake = hooks.use_ref(|| Arc::new(Notify::new()));
    let mut was_pulsing = hooks.use_ref(|| false);
    if props.pulse && !was_pulsing.get() {
        wake.read().notify_one();
    }
    was_pulsing.set(props.pulse);

    let wake_fut = wake.read().clone();
    hooks.use_future(async move {
        loop {
            // Steady dot: park until it starts pulsing again, so a screen full
            // of running processes wakes no timers.
            if !pulse_active.try_get().unwrap_or(false) {
                wake_fut.notified().await;
                continue;
            }
            tokio::time::sleep(Duration::from_millis(PULSE_INTERVAL_MS)).await;
            // Re-check after sleeping. Use `try_get` (not the panicking `get`)
            // so a dropped owner ends the loop cleanly, matching `bright` below.
            let Some(active) = pulse_active.try_get() else {
                break;
            };
            if !active {
                continue;
            }
            let Some(val) = bright.try_get() else {
                break;
            };
            bright.set(!val);
        }
    });

    let shown = if props.pulse && !bright.get() {
        COLOR_HIERARCHY
    } else {
        color
    };
    element!(Text(content: props.glyph.clone(), color: shown))
}

/// Build logs viewport height for collapsed preview (press 'e' to expand to fullscreen)
pub const LOG_VIEWPORT_COLLAPSED: usize = 10;
/// Viewport height for failed activities (show more context on failure)
pub const LOG_VIEWPORT_FAILED: usize = 20;
/// Reduced viewport height for tasks with showOutput=true (expands to full when selected)
pub const LOG_VIEWPORT_SHOW_OUTPUT: usize = 3;
/// Hard cap on the inline log preview's visual-row footprint. Long log lines
/// wrap onto continuation rows, so the visual height can exceed `max_lines`;
/// without a cap, e.g. 20 long lines × several wrap rows each would push the
/// surrounding activity tree off-screen. Pressing 'e' opens the expanded view
/// for unconstrained scrolling.
pub const INLINE_LOG_MAX_VISUAL_ROWS: u32 = 50;

/// Keep activity data from becoming terminal control input. Newlines and tabs
/// are layout, not content, in single-row fields; other C0/C1 controls are
/// equally unsafe and are rendered as spaces.
pub(crate) fn sanitize_inline_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
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
    pub ancestor_continuations: Vec<bool>,
    pub is_last_sibling: bool,
}

impl HierarchyPrefixComponent {
    pub fn new(depth: usize, ancestor_continuations: &[bool], is_last_sibling: bool) -> Self {
        Self {
            depth,
            ancestor_continuations: ancestor_continuations.to_vec(),
            is_last_sibling,
        }
    }

    /// Renders the hierarchy prefix: `[indent][branch]`
    /// The indent aligns with parent's content (after their status indicator).
    pub fn render(&self) -> Vec<AnyElement<'static>> {
        if self.depth == 0 {
            return vec![];
        }

        let mut total_indent = "  ".to_string();
        for continues in &self.ancestor_continuations {
            total_indent.push_str(if *continues { "│ " } else { "  " });
        }
        let branch = if self.is_last_sibling { "└" } else { "├" };

        vec![
            element!(Text(content: total_indent, color: COLOR_HIERARCHY)).into_any(),
            element!(View(margin_right: 1) {
                Text(content: branch, color: COLOR_HIERARCHY)
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
    pub is_dimmed: bool,
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
            is_dimmed: false,
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

    pub fn with_dimmed(mut self, is_dimmed: bool) -> Self {
        self.is_dimmed = is_dimmed;
        self
    }

    pub fn with_completed(mut self, completed: bool) -> Self {
        self.is_completed = completed;
        self
    }

    fn colors(&self, depth: usize) -> (Color, Color, Color, Option<Color>) {
        if self.is_selected {
            (
                Color::AnsiValue(232),
                Color::AnsiValue(238),
                Color::AnsiValue(238),
                Some(Color::AnsiValue(250)),
            )
        } else if self.is_dimmed {
            (COLOR_HIERARCHY, COLOR_HIERARCHY, COLOR_HIERARCHY, None)
        } else if self.is_completed && depth == 0 {
            (Color::Reset, COLOR_SECONDARY, COLOR_HIERARCHY, None)
        } else if self.is_completed {
            (COLOR_ACTIVE_NESTED, COLOR_SECONDARY, COLOR_HIERARCHY, None)
        } else if depth == 0 || matches!(self.variant, ActivityVariant::Process(_)) {
            (COLOR_ACTIVE, COLOR_SECONDARY, COLOR_HIERARCHY, None)
        } else {
            (COLOR_ACTIVE_NESTED, COLOR_SECONDARY, COLOR_HIERARCHY, None)
        }
    }

    pub fn render(
        &self,
        terminal_width: u16,
        depth: usize,
        prefix_children: Vec<AnyElement<'static>>,
    ) -> AnyElement<'static> {
        let name = sanitize_inline_text(&self.name);
        let action = sanitize_inline_text(&self.action);
        let suffix = self
            .suffix
            .as_deref()
            .map(sanitize_inline_text)
            .filter(|suffix| !suffix.is_empty());
        // iocraft's flex row retains one trailing cell beyond the explicit
        // child budgets, so reserve it before deciding how much name/suffix
        // content can be rendered.
        let (shortened_name, display_suffix) = calculate_display_info(
            &name,
            terminal_width.saturating_sub(1) as u32,
            &action,
            suffix.as_deref(),
            &self.elapsed,
            depth,
        );

        let (name_color, suffix_color, elapsed_color, bg_color) = self.colors(depth);

        let mut final_prefix = prefix_children;

        // Only add action text if action is not empty
        if !action.is_empty() {
            // Action word should be capitalized
            let action_text = {
                let mut chars = action.chars();
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
                View(height: 1, flex_direction: FlexDirection::Row, padding_right: 1, background_color: bg) {
                    // Fixed left column - never truncates
                    View(flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                        #(final_prefix)
                    }
                    // Flexible middle column - can overflow
                    // Each item uses leading margin (margin_left) to separate from predecessor
                    View(flex_grow: 1.0_f32, min_width: 0, overflow: Overflow::Hidden, margin_right: 1, flex_direction: FlexDirection::Row) {
                        #(if !shortened_name.is_empty() {
                            let has_predecessor = !action.is_empty();
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
                View(height: 1, flex_direction: FlexDirection::Row, padding_right: 1) {
                    // Fixed left column - never truncates
                    View(flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                        #(final_prefix)
                    }
                    // Flexible middle column - can overflow
                    // Each item uses leading margin (margin_left) to separate from predecessor
                    View(flex_grow: 1.0_f32, min_width: 0, overflow: Overflow::Hidden, margin_right: 1, flex_direction: FlexDirection::Row) {
                        #(if !shortened_name.is_empty() {
                            let has_predecessor = !action.is_empty();
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

        // Clamp to bar_width: progress can exceed 100% (e.g. reported bytes past
        // the expected total), which would otherwise underflow `empty`.
        let filled = ((bar_width * self.percent as usize) / 100).min(bar_width);
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
    pub ancestor_continuations: Vec<bool>,
    pub is_last_sibling: bool,
}

impl<'a> DownloadActivityComponent<'a> {
    pub fn new(activity: &'a Activity, depth: usize, is_selected: bool) -> Self {
        Self {
            activity,
            depth,
            is_selected,
            completed: None,
            cached: false,
            ancestor_continuations: Vec::new(),
            is_last_sibling: true,
        }
    }

    pub fn with_hierarchy(
        mut self,
        ancestor_continuations: &[bool],
        is_last_sibling: bool,
    ) -> Self {
        self.ancestor_continuations = ancestor_continuations.to_vec();
        self.is_last_sibling = is_last_sibling;
        self
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
        let mut prefix = HierarchyPrefixComponent::new(
            self.depth,
            &self.ancestor_continuations,
            self.is_last_sibling,
        )
        .render();
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
    let suffix = suffix.filter(|suffix| !suffix.is_empty());
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
    let padding_width = 1; // activity rows reserve one column on the right
    // Every activity has a two-column status indicator (glyph + margin), at
    // every depth. Previously nested rows omitted it from the budget.
    let status_width = 2;

    let base_width = padding_width
        + indent_width
        + hierarchy_width
        + status_width
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

    // Fallback: if it still looks like a hash, truncate and add ellipsis.
    // Truncate by characters, not bytes, so multi-byte UTF-8 never panics.
    if path.len() > 15 && path.chars().all(|c| c.is_alphanumeric()) {
        // Looks like just a hash, truncate to first few chars + ellipsis
        let prefix: String = path.chars().take(4).collect();
        format!("{}…", prefix)
    } else if path.len() > 20 {
        // For file paths (like evaluation paths), keep the end and truncate the beginning
        if path.contains('/') {
            let chars: Vec<char> = path.chars().collect();
            let tail: String = chars[chars.len().saturating_sub(19)..].iter().collect();
            format!("…{}", tail)
        } else {
            let prefix: String = path.chars().take(19).collect();
            format!("{}…", prefix)
        }
    } else {
        path.to_string()
    }
}

/// Component for rendering collapsed content preview (logs, details, traces) inline below activities.
/// Press 'e' to expand to fullscreen view with scrolling.
pub struct ExpandedContentComponent<'a> {
    pub lines: Option<&'a VecDeque<String>>,
    pub empty_message: &'a str,
    pub max_lines: usize,
    pub depth: usize,
    pub terminal_width: Option<usize>,
    border_indent: Option<usize>,
    line_prefix: Option<String>,
}

impl<'a> ExpandedContentComponent<'a> {
    pub fn new(lines: Option<&'a VecDeque<String>>) -> Self {
        Self {
            lines,
            empty_message: "  → no content",
            max_lines: LOG_VIEWPORT_COLLAPSED,
            depth: 0,
            terminal_width: None,
            border_indent: None,
            line_prefix: None,
        }
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

    pub fn with_terminal_width(mut self, width: u16) -> Self {
        self.terminal_width = Some(width as usize);
        self
    }

    pub fn with_border_indent(mut self, indent: usize) -> Self {
        self.border_indent = Some(indent);
        self
    }

    pub fn with_line_prefix(mut self, prefix: String) -> Self {
        self.line_prefix = Some(prefix);
        self
    }

    fn border_indent(&self) -> usize {
        self.border_indent.unwrap_or(2 + self.depth * 2)
    }

    /// The last `max_lines` logical log lines, preserving chronological order.
    /// Long unbroken strings are hard-wrapped here because a `Text` child's
    /// intrinsic width can otherwise expand its parent beyond the terminal.
    fn visible_lines(&self) -> Vec<String> {
        let Some(lines) = self.lines else {
            return Vec::new();
        };
        let lines: Vec<_> = lines
            .iter()
            .rev()
            .take(self.max_lines)
            .rev()
            .cloned()
            .collect();
        let Some(terminal_width) = self.terminal_width else {
            return lines;
        };

        let occupied_width = self.line_prefix.as_ref().map_or_else(
            || self.border_indent() + 3,
            |prefix| {
                use unicode_width::UnicodeWidthStr;
                prefix.width() + 3
            },
        );
        let width = terminal_width.saturating_sub(occupied_width).max(1);
        lines
            .into_iter()
            .flat_map(|line| hard_wrap(&line, width))
            .collect()
    }

    pub fn visual_height(&self) -> usize {
        self.visible_lines()
            .len()
            .max(1)
            .min(INLINE_LOG_MAX_VISUAL_ROWS.saturating_sub(1) as usize)
    }

    pub fn render(&self) -> Vec<AnyElement<'static>> {
        let indent = self.border_indent();

        let lines = self.visible_lines();
        if !lines.is_empty() {
            if let Some(prefix) = &self.line_prefix {
                let line_elements: Vec<AnyElement<'static>> = lines
                    .into_iter()
                    .map(|line| {
                        element! {
                            View(flex_direction: FlexDirection::Row, padding_right: 1) {
                                Text(content: prefix.clone(), color: COLOR_HIERARCHY)
                                Text(content: format!("  {line}"), color: Color::AnsiValue(245))
                            }
                        }
                        .into_any()
                    })
                    .collect();

                return vec![
                    element! {
                    View(
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::Hidden,
                    ) {
                        #(line_elements)
                        }
                    }
                    .into_any(),
                ];
            }

            let line_elements: Vec<AnyElement<'static>> = lines
                .into_iter()
                .map(|line| {
                    element! {
                        Text(content: format!(" {line}"), color: Color::AnsiValue(245))
                    }
                    .into_any()
                })
                .collect();

            return vec![
                element! {
                    View(
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::Hidden,
                        margin_left: indent as u32,
                        margin_right: 1,
                        border_style: BorderStyle::Single,
                        border_edges: Edges::Left,
                        border_color: COLOR_HIERARCHY,
                    ) {
                        #(line_elements)
                    }
                }
                .into_any(),
            ];
        }

        // Fallback: show an empty-state hint on one row. Bound it explicitly;
        // unbroken hint text has the same intrinsic-width behavior as logs.
        let empty_message = self
            .terminal_width
            .and_then(|width| {
                let budget = width.saturating_sub(indent + 1).max(1);
                hard_wrap(self.empty_message, budget).into_iter().next()
            })
            .unwrap_or_else(|| self.empty_message.to_string());
        if let Some(prefix) = &self.line_prefix {
            return vec![
                element! {
                    View(height: 1, flex_direction: FlexDirection::Row, padding_right: 1) {
                        Text(content: prefix.clone(), color: COLOR_HIERARCHY)
                        Text(content: format!("  {empty_message}"), color: Color::AnsiValue(245))
                    }
                }
                .into_any(),
            ];
        }
        vec![element! {
            View(height: 1, flex_direction: FlexDirection::Column, padding_left: indent as u32, padding_right: 1) {
                Text(content: empty_message, color: Color::AnsiValue(245))
            }
        }
        .into_any()]
    }

    /// Approximate visible height: one row per log line. The actual height after
    /// `Text` wrapping may be larger, so callers that bound the inline view should
    /// rely on a flex `max_height` rather than treating this as an upper bound.
    pub fn calculate_height(&self) -> usize {
        let count = self.visible_lines().len();
        if count > 0 { count } else { 1 }
    }

    /// Render the component with a main activity line. Height is content-sized
    /// up to `INLINE_LOG_MAX_VISUAL_ROWS` so wrapped log lines aren't clipped
    /// but a runaway wrap can't push the rest of the activity tree off-screen.
    pub fn render_with_main_line(&self, main_line: AnyElement<'static>) -> AnyElement<'static> {
        let mut elements = vec![main_line];
        elements.extend(self.render());

        element! {
            View(
                flex_direction: FlexDirection::Column,
                max_height: INLINE_LOG_MAX_VISUAL_ROWS,
                overflow: Overflow::Hidden,
            ) {
                #(elements)
            }
        }
        .into_any()
    }
}

fn hard_wrap(line: &str, max_width: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;

    let mut wrapped = Vec::new();
    for logical_line in line.split('\n') {
        let mut current = String::new();
        let mut width = 0;
        for character in logical_line.chars() {
            let character = if character.is_control() {
                ' '
            } else {
                character
            };
            let character_width = character.width().unwrap_or(0);
            if width > 0 && width + character_width > max_width {
                wrapped.push(std::mem::take(&mut current));
                width = 0;
            }
            current.push(character);
            width += character_width;
        }
        wrapped.push(current);
    }
    wrapped
}

/// Backwards-compatible alias
pub type BuildLogsComponent<'a> = ExpandedContentComponent<'a>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimmed_activity_uses_hierarchy_colors() {
        let component = ActivityTextComponent::name_only(
            "email-worker".to_string(),
            "1.0s".to_string(),
            ActivityVariant::Process(crate::model::ProcessActivity {
                status: ProcessStatus::Ready,
                ports: vec![],
                urls: vec![],
                ready_probe: None,
            }),
        )
        .with_dimmed(true);

        assert_eq!(
            component.colors(1),
            (COLOR_HIERARCHY, COLOR_HIERARCHY, COLOR_HIERARCHY, None)
        );
        assert_eq!(
            component.with_selection(true).colors(1),
            (
                Color::AnsiValue(232),
                Color::AnsiValue(238),
                Color::AnsiValue(238),
                Some(Color::AnsiValue(250))
            )
        );
    }

    #[test]
    fn process_status_dot_covers_every_lifecycle_phase() {
        for (status, expected) in [
            (
                ProcessStatus::NotStarted,
                (DOT_INERT, COLOR_HIERARCHY, false),
            ),
            (ProcessStatus::Waiting, (DOT_RING, COLOR_TRANSIENT, true)),
            (ProcessStatus::Starting, (DOT_HALF, COLOR_TRANSIENT, true)),
            (
                ProcessStatus::Running,
                (DOT_RUNNING, COLOR_COMPLETED, false),
            ),
            (ProcessStatus::Ready, (DOT_READY, COLOR_COMPLETED, false)),
            (ProcessStatus::Restarting, (DOT_HALF, COLOR_TRANSIENT, true)),
            (ProcessStatus::Stopping, (DOT_HALF, COLOR_HIERARCHY, true)),
            (ProcessStatus::Stopped, (DOT_RING, COLOR_HIERARCHY, false)),
            (ProcessStatus::Exited, (DOT_RING, COLOR_HIERARCHY, false)),
            (ProcessStatus::GaveUp, (XMARK, COLOR_FAILED, false)),
        ] {
            assert_eq!(
                process_status_dot(&status, None, false),
                expected,
                "{status:?}"
            );
        }

        assert_eq!(
            process_status_dot(&ProcessStatus::Ready, None, true),
            (DOT_HALF, COLOR_HIERARCHY, true),
            "global shutdown overrides every active phase"
        );
        assert_eq!(
            process_status_dot(&ProcessStatus::GaveUp, None, true),
            (XMARK, COLOR_FAILED, false),
            "GaveUp remains a stable failure during global shutdown"
        );
    }

    // For depth=0, action="", elapsed="1.0s":
    // base_width = padding(1) + status(2) + action(0+1) + name_margin(1) + elapsed(4) = 9
    // remaining = terminal_width - 9

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
        // remaining = 60 - 9 = 51, name=24
        // space_for_suffix = 51 - 24 - 1(margin) = 26 chars
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
        assert_eq!(suffix.chars().count(), 26);
    }

    #[test]
    fn suffix_dropped_when_only_1_char_available() {
        // remaining = 35 - 9 = 26, name=24
        // space_for_suffix = 26 - 24 - 1(margin) = 1 char, which is < 2 so suffix is dropped
        let (name, suffix) = calculate_display_info(
            "devenv:python:virtualenv",
            35,
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
        // remaining = 20 - 9 = 11, name=24 doesn't fit and is left-truncated
        let (name, suffix) =
            calculate_display_info("devenv:python:virtualenv", 20, "", None, "1.0s", 0);
        assert!(name.starts_with('…'));
        assert_eq!(name.chars().count(), 11);
        assert_eq!(suffix, None);
    }

    #[test]
    fn name_truncated_after_suffix_dropped() {
        // remaining = 20 - 9 = 11; suffix is dropped, then the name is truncated
        let (name, suffix) = calculate_display_info(
            "devenv:python:virtualenv",
            20,
            "",
            Some("4 lines"),
            "1.0s",
            0,
        );
        assert!(name.starts_with('…'));
        assert_eq!(name.chars().count(), 11);
        assert_eq!(suffix, None);
    }

    #[test]
    fn very_constrained_uses_aggressive_shortening() {
        // base=9 >= terminal=5, hits shorten_store_path_aggressive
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
        // remaining = 13 - 9 = 4, which is <= 4 so just "…"
        let (name, suffix) =
            calculate_display_info("devenv:python:virtualenv", 13, "", None, "1.0s", 0);
        assert_eq!(name, "…");
        assert_eq!(suffix, None);
    }

    #[test]
    fn nesting_reduces_budget() {
        // depth=0: base = 9, remaining = 33
        // depth=2 includes indent(4), hierarchy(2), and status(2): base = 15,
        //          remaining = 27
        // name(24) + suffix("cached" 6+1margin) = 31
        // depth=0 fits; depth=2 leaves two columns for a truncated suffix.
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
        assert_eq!(s.chars().count(), 2);
    }

    #[test]
    fn action_reduces_budget() {
        // action="building"(8+1=9)
        // base = padding(1)+status(2)+action(9)+name_margin(1)+elapsed(4) = 17
        // remaining = 43; after name and margin the suffix gets 30 columns.
        let long_suffix = "4 lines → DEVENV_EXPORT:VklSVFVBTF9FTlY==L2hvbWUvZG9tZW4";
        let (name, suffix) =
            calculate_display_info("some-package", 60, "building", Some(long_suffix), "1.0s", 0);
        assert_eq!(name, "some-package");
        let suffix = suffix.expect("suffix should be truncated");
        assert!(suffix.ends_with('…'));
        assert_eq!(suffix.chars().count(), 30);
    }

    #[test]
    fn multibyte_suffix_does_not_panic() {
        // Suffix with multi-byte UTF-8 chars: .len() (bytes) > .chars().count()
        // This previously panicked because byte length was used for comparison
        // but char index was used for slicing.
        // "ä" is 2 bytes in UTF-8, so 30 chars = 60 bytes.
        // With terminal_width=80, action="building", path="pkg":
        //   base_width = 1+2+9+1+4 = 17, remaining = 63
        //   space_for_suffix = 63 - 3 - 1 = 59
        //   Old code: 58 >= suffix.len()(60) → false, then chars[..57] on 30-char vec → panic!
        let suffix = "ääääääääääääääääääääääääääääää"; // 30 chars, 60 bytes
        let (name, _suffix) =
            calculate_display_info("pkg", 80, "building", Some(suffix), "1.0s", 0);
        assert_eq!(name, "pkg");
    }

    #[test]
    fn expanded_content_keeps_last_max_lines_in_chronological_order() {
        let mut logs = VecDeque::new();
        logs.push_back("one".to_string());
        logs.push_back("two".to_string());
        logs.push_back("three".to_string());
        logs.push_back("four".to_string());

        let component = ExpandedContentComponent::new(Some(&logs)).with_max_lines(2);

        assert_eq!(
            component.visible_lines(),
            vec!["three".to_string(), "four".to_string()]
        );
    }

    #[test]
    fn expanded_content_passes_long_lines_through_unchanged() {
        // Without a terminal width (e.g. standalone component use), preserve
        // the old behavior and let the parent renderer choose a width.
        let long = "INFO    -  [12:00:00] Serving on http://127.0.0.1:8000/".to_string();
        let mut logs = VecDeque::new();
        logs.push_back(long.clone());

        let component = ExpandedContentComponent::new(Some(&logs)).with_max_lines(3);

        assert_eq!(component.visible_lines(), vec![long]);
    }

    #[test]
    fn expanded_content_hard_wraps_unbroken_wide_text_to_its_budget() {
        let mut logs = VecDeque::new();
        logs.push_back("aaaaaaaaaaaaaaaaaaaa界界".to_string());

        // Width 20 minus the component's five columns of framing gives each
        // content chunk a 15-column budget.
        let component = ExpandedContentComponent::new(Some(&logs)).with_terminal_width(20);
        let lines = component.visible_lines();

        assert_eq!(lines, vec!["aaaaaaaaaaaaaaa", "aaaaa界界"]);
    }

    #[test]
    fn inline_log_view_wraps_long_url_at_narrow_terminal() {
        // Reproduce the user's reported scenario: at a narrow terminal width a
        // long mkdocs-style URL line should wrap onto continuation rows with
        // every char of the original line still present.
        let mut logs = VecDeque::new();
        let line = "INFO    -  [13:23:16] Serving on http://127.0.0.1:8000/".to_string();
        logs.push_back(line.clone());

        let component = ExpandedContentComponent::new(Some(&logs))
            .with_max_lines(3)
            .with_depth(1);

        let elements = component.render();
        let mut elem = element! {
            View(width: 40u32, flex_direction: FlexDirection::Column) {
                #(elements)
            }
        };
        let out = elem.render(Some(40)).to_string();

        // No ellipsis truncation, and every char of the URL appears somewhere
        // in the rendered output (possibly across wrap rows).
        assert!(
            !out.contains('…'),
            "rendered output contained ellipsis:\n{}",
            out
        );
        // The most distinctive tail of the URL should appear unmodified on
        // some visual row (iocraft wraps at the last word boundary so "/"
        // ends up on its own row after "Serving on").
        assert!(
            out.contains("http://127.0.0.1:8000/"),
            "expected URL to appear intact in wrapped output:\n{}",
            out
        );
    }

    #[test]
    fn expanded_content_supports_explicit_border_alignment() {
        let logs = VecDeque::from(["one".to_string()]);
        let elements = ExpandedContentComponent::new(Some(&logs))
            .with_border_indent(2)
            .render();
        let mut element = element! {
            View(width: 40u32, flex_direction: FlexDirection::Column) {
                #(elements)
            }
        };
        let rendered = element.render(Some(40)).to_string();

        assert!(rendered.contains("  │ one"), "{rendered}");
    }
}
