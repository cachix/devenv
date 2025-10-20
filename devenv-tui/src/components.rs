//! Reusable UI components for the TUI

use crate::model::{Activity, ActivityVariant};
use human_repr::{HumanCount, HumanThroughput};
use iocraft::prelude::*;
use std::collections::VecDeque;
use std::time::Instant;

/// Spinner animation frames (matching current CLI)
pub const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Build logs viewport height when collapsed
const LOG_VIEWPORT_COLLAPSED: usize = 10;

/// Build logs viewport height when expanded
const LOG_VIEWPORT_EXPANDED: usize = 100;

/// Color constants for operations
pub const COLOR_ACTIVE: Color = Color::Rgb {
    r: 0,
    g: 128,
    b: 157,
}; // #00809D - Nice blue for active/in-progress
pub const COLOR_COMPLETED: Color = Color::Rgb {
    r: 112,
    g: 138,
    b: 88,
}; // #708A58 - Sage green for completed/done
pub const COLOR_FAILED: Color = Color::AnsiValue(160); // Nice bright red for failed
pub const COLOR_INTERACTIVE: Color = Color::Rgb {
    r: 255,
    g: 215,
    b: 0,
}; // #FFD700 - Gold for selected items and UI hints
pub const COLOR_HIERARCHY: Color = Color::AnsiValue(242); // Medium grey for hierarchy indicators

/// Component for building consistent hierarchy prefix for activities
pub struct HierarchyPrefixComponent {
    pub indent: String,
    pub depth: usize,
    pub spinner: Option<String>,
}

impl HierarchyPrefixComponent {
    pub fn new(indent: String, depth: usize) -> Self {
        Self {
            indent,
            depth,
            spinner: None,
        }
    }

    pub fn with_spinner(mut self, spinner_frame: usize) -> Self {
        self.spinner = Some(SPINNER_FRAMES[spinner_frame].to_string());
        self
    }

    pub fn render(&self) -> Vec<AnyElement<'static>> {
        let mut prefix_children = vec![];

        // Add hierarchy indicator if indented
        if self.depth > 0 {
            // Show parent level indentation (depth-1) * 2 spaces
            let parent_indent = "  ".repeat(self.depth - 1);
            prefix_children.push(element!(Text(content: parent_indent)).into_any());
            prefix_children.push(
                element!(View(margin_right: 1) {
                    Text(content: "└──", color: COLOR_HIERARCHY)
                })
                .into_any(),
            );
        } else {
            // Top-level item, use the full indent
            prefix_children.push(element!(Text(content: self.indent.clone())).into_any());
        }

        // Show spinner for top-level items (depth == 0)
        if self.depth == 0
            && let Some(ref spinner_char) = self.spinner
        {
            prefix_children.push(
                element!(View(margin_right: 1) {
                    Text(content: spinner_char, color: COLOR_ACTIVE)
                })
                .into_any(),
            );
        }

        prefix_children
    }
}

/// Component for rendering colored activity text
pub struct ActivityTextComponent {
    pub action: String,
    pub name: String,
    pub suffix: Option<String>,
    pub is_selected: bool,
    pub elapsed: String,
}

impl ActivityTextComponent {
    pub fn new(action: String, name: String, elapsed: String) -> Self {
        Self {
            action,
            name,
            suffix: None,
            is_selected: false,
            elapsed,
        }
    }

    pub fn with_suffix(mut self, suffix: Option<String>) -> Self {
        self.suffix = suffix;
        self
    }

    pub fn with_selection(mut self, is_selected: bool) -> Self {
        self.is_selected = is_selected;
        self
    }

    pub fn render(
        &self,
        terminal_width: u16,
        depth: usize,
        prefix_children: Vec<AnyElement<'static>>,
    ) -> AnyElement<'static> {
        let (shortened_name, show_suffix) = calculate_display_info(
            &self.name,
            terminal_width as u32,
            &self.action,
            self.suffix.as_deref(),
            &self.elapsed,
            depth,
        );

        // Action word should be capitalized (handle empty strings)
        let action_text = if self.action.is_empty() {
            String::new()
        } else {
            format!(
                "{}{}",
                self.action
                    .chars()
                    .next()
                    .unwrap_or_default()
                    .to_uppercase()
                    .collect::<String>(),
                &self.action[1..]
            )
        };
        let mut final_prefix = prefix_children;
        final_prefix.push(
            element!(View(width: (action_text.len() + 1) as u32, flex_shrink: 0.0) {
                View(margin_right: 1) {
                    Text(content: action_text, color: COLOR_ACTIVE, weight: Weight::Bold)
                }
            })
            .into_any(),
        );

        element! {
            View(height: 1, flex_direction: FlexDirection::Row, padding_left: 1, padding_right: 1) {
                // Fixed left column - never truncates
                View(flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                    #(final_prefix)
                }
                // Flexible middle column - can overflow  
                View(flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden, margin_right: 1, flex_direction: FlexDirection::Row) {
                    Text(content: shortened_name, color: if self.is_selected { COLOR_INTERACTIVE } else { Color::Reset })
                    #(if show_suffix && self.suffix.is_some() {
                        vec![element!(View(margin_left: 1) {
                            Text(content: self.suffix.as_ref().expect("suffix should be Some when show_suffix is true"), color: COLOR_HIERARCHY)
                        }).into_any()]
                    } else {
                        vec![]
                    })
                }
                // Fixed right column - never truncates
                View(flex_shrink: 0.0) {
                    Text(content: self.elapsed.clone(), color: Color::AnsiValue(242))
                }
            }
        }
        .into()
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
                Text(content: size_info, color: Color::AnsiValue(242))
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
    pub spinner_frame: usize,
}

impl<'a> DownloadActivityComponent<'a> {
    pub fn new(
        activity: &'a Activity,
        depth: usize,
        is_selected: bool,
        spinner_frame: usize,
    ) -> Self {
        Self {
            activity,
            depth,
            is_selected,
            spinner_frame,
        }
    }

    pub fn render(&self, terminal_width: u16) -> AnyElement<'static> {
        let indent = "  ".repeat(self.depth);
        let elapsed = Instant::now().duration_since(self.activity.start_time);
        let elapsed_str = format!("{:.1}s", elapsed.as_secs_f64());

        let mut elements = vec![];

        // First line: activity name
        let prefix = HierarchyPrefixComponent::new(indent.clone(), self.depth).render();

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

        let mut line1_children = prefix;
        line1_children.extend(vec![
            element!(View(margin_right: 1) {
                Text(content: "Downloading", color: COLOR_ACTIVE, weight: Weight::Bold)
            }).into_any(),
            element!(View(margin_right: 1) {
                Text(content: shortened_name, color: if self.is_selected { COLOR_INTERACTIVE } else { Color::Reset })
            }).into_any(),
        ]);

        if let Some(substituter) = &substituter {
            // Only show "from" text on wider terminals
            if terminal_width >= 80 {
                line1_children.push(
                    element!(Text(content: format!("from {}", substituter), color: COLOR_HIERARCHY))
                        .into_any(),
                );
            }
        }

        elements.push(
            element! {
                View(height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, width: 100pct, padding_left: 1, padding_right: 1, overflow: Overflow::Hidden) {
                    View(flex_direction: FlexDirection::Row, width: 100pct, overflow: Overflow::Hidden) {
                        #(line1_children)
                    }
                    View {
                        Text(content: elapsed_str, color: Color::AnsiValue(242))
                    }
                }
            }
            .into_any()
        );

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

/// Calculate display info for activity considering terminal width
pub fn calculate_display_info(
    path: &str,
    terminal_width: u32,
    action: &str,
    suffix: Option<&str>,
    elapsed: &str,
    depth: usize,
) -> (String, bool) {
    // Calculate base width without suffix: padding + indent + hierarchy + action + margin + name + margin + elapsed
    let indent_width = if depth > 0 {
        (depth - 1) * 2
    } else {
        depth * 2
    }; // parent levels only
    let hierarchy_width = if depth > 0 { 4 } else { 0 }; // "└──" + margin_right: 1 for indented items
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
        return (shorten_store_path_aggressive(path), false);
    }

    let remaining_width_without_suffix = available_width - base_width;

    // Check if we can show suffix
    let suffix_width = suffix.map(|s| s.len() + 1).unwrap_or(0); // suffix + space prefix
    let show_suffix = suffix_width <= remaining_width_without_suffix / 3; // Only show suffix if it takes less than 1/3 of remaining space

    let remaining_width_for_path = if show_suffix {
        remaining_width_without_suffix - suffix_width
    } else {
        remaining_width_without_suffix
    };

    // If path fits in remaining width, don't shorten
    if path.len() <= remaining_width_for_path {
        return (path.to_string(), show_suffix);
    }

    // Path doesn't fit - truncate from the left to keep meaningful filename
    if remaining_width_for_path > 4 {
        // Use char indices to avoid slicing in the middle of UTF-8 characters
        let chars: Vec<char> = path.chars().collect();
        let start_char = chars.len().saturating_sub(remaining_width_for_path - 1);
        let truncated_chars: String = chars.iter().skip(start_char).collect();
        let truncated = format!("…{}", truncated_chars);
        return (truncated, show_suffix);
    }

    // If extremely narrow, just show ellipsis
    ("…".to_string(), false)
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

/// Component for rendering build logs inline below build activities
pub struct BuildLogsComponent<'a> {
    pub logs: Option<&'a VecDeque<String>>,
    pub expanded: bool,
}

impl<'a> BuildLogsComponent<'a> {
    pub fn new(logs: Option<&'a VecDeque<String>>, expanded: bool) -> Self {
        Self { logs, expanded }
    }

    pub fn render(&self) -> Vec<AnyElement<'static>> {
        let max_viewport_height = if self.expanded {
            LOG_VIEWPORT_EXPANDED
        } else {
            LOG_VIEWPORT_COLLAPSED
        };

        if let Some(logs) = &self.logs
            && !logs.is_empty()
        {
            // Take the last N lines that fit in viewport
            let log_lines: Vec<_> = logs.iter().rev().take(max_viewport_height).rev().collect();

            if !log_lines.is_empty() {
                // Use actual number of log lines, not fixed viewport height
                let actual_height = log_lines.len();

                // Create elements for all log lines
                let mut log_elements = vec![];
                for line in log_lines {
                    log_elements.push(element! {
                            View(height: 1, flex_direction: FlexDirection::Row, padding_left: 2, padding_right: 1) {
                                Text(content: line.clone(), color: Color::AnsiValue(245))
                            }
                        }.into_any());
                }

                // Return a single container with actual height of log lines
                return vec![element! {
                        View(height: actual_height as u32, flex_direction: FlexDirection::Column, overflow: Overflow::Hidden) {
                            #(log_elements)
                        }
                    }.into_any()];
            }
        }

        // Fallback: show "no logs" message with minimal height
        vec![element! {
            View(height: 1, flex_direction: FlexDirection::Column, padding_left: 2, padding_right: 1) {
                Text(content: "  → no build logs yet".to_string(), color: Color::AnsiValue(245))
            }
        }.into_any()]
    }

    /// Calculate the height this component will take
    pub fn calculate_height(&self) -> usize {
        let max_viewport_height = if self.expanded {
            LOG_VIEWPORT_EXPANDED
        } else {
            LOG_VIEWPORT_COLLAPSED
        };

        if let Some(logs) = &self.logs
            && !logs.is_empty()
        {
            let log_lines: Vec<_> = logs.iter().rev().take(max_viewport_height).rev().collect();
            if !log_lines.is_empty() {
                return log_lines.len();
            }
        }

        // Fallback: minimal height for "no logs" message
        1
    }
}
