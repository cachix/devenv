use crate::{
    model::{ActivityInfo, ActivitySummary, Model},
    NixActivityType,
};
use human_repr::{HumanCount, HumanDuration, HumanThroughput};
use iocraft::prelude::*;
use std::collections::VecDeque;
use std::time::Duration;

/// Spinner animation frames (matching current CLI)
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Color constants for operations
const COLOR_ACTIVE: Color = Color::Rgb {
    r: 0,
    g: 128,
    b: 157,
}; // #00809D - Nice blue for active/in-progress
const COLOR_COMPLETED: Color = Color::Rgb {
    r: 112,
    g: 138,
    b: 88,
}; // #708A58 - Sage green for completed/done
const COLOR_FAILED: Color = Color::AnsiValue(160); // Nice bright red for failed
const COLOR_INTERACTIVE: Color = Color::Rgb {
    r: 255,
    g: 215,
    b: 0,
}; // #FFD700 - Gold for selected items and UI hints
const COLOR_HIERARCHY: Color = Color::AnsiValue(242); // Medium grey for hierarchy indicators

/// Main view function that creates the UI
pub fn view(model: &Model) -> impl Into<AnyElement<'static>> {
    let active_activities = model.get_active_activities();

    let summary = model.calculate_summary();
    let has_selection = model.ui.selected_activity_index.is_some();
    let spinner_frame = model.ui.spinner_frame;
    let selected_index = model.ui.selected_activity_index;

    // Create owned activity elements
    let activity_elements: Vec<_> = active_activities
        .into_iter()
        .enumerate()
        .map(|(idx, activity)| {
            render_activity_owned(
                activity,
                idx == selected_index.unwrap_or(usize::MAX),
                spinner_frame,
            )
        })
        .collect();

    // Check if we have a selected build activity with logs
    let selected_activity = model.get_selected_activity();
    let build_logs = selected_activity
        .as_ref()
        .filter(|a| matches!(a.activity_type, NixActivityType::Build))
        .and_then(|a| a.activity_id)
        .and_then(|id| model.get_build_logs(id));

    let summary_view = build_summary_view(
        &summary,
        has_selection,
        model.ui.show_expanded_logs,
        build_logs.is_some(),
    );

    let show_expanded_logs = model.ui.show_expanded_logs;

    // Calculate dynamic height based on number of activities
    let activity_count = activity_elements.len();
    let min_height = 3; // Minimum height to show at least a few items
    let max_height = 20; // Maximum height to prevent taking too much screen
    let mut dynamic_height = activity_count.clamp(min_height, max_height) as u32;

    // Add height for logs if showing
    if let Some(logs) = &build_logs {
        if show_expanded_logs {
            // Show all logs (up to a reasonable max)
            let log_lines = logs.len().min(50); // Cap at 50 lines max
            dynamic_height += (log_lines + 1) as u32; // +1 for separator
        } else {
            let log_lines = logs.len().min(10); // Show up to 10 lines
            dynamic_height += (log_lines + 1) as u32; // +1 for separator
        }
    }

    let mut children = vec![];

    // Activity list - only use flex_grow when no logs are shown
    if build_logs.is_some() {
        children.push(
            element! {
                View(margin_left: 1) {
                    View(flex_direction: FlexDirection::Column) {
                        #(activity_elements)
                    }
                }
            }
            .into_any(),
        );
    } else {
        children.push(
            element! {
                View(flex_grow: 1.0, margin_left: 1) {
                    View(flex_direction: FlexDirection::Column) {
                        #(activity_elements)
                    }
                }
            }
            .into_any(),
        );
    }

    // Add build logs if a build is selected
    if let Some(logs) = build_logs {
        children.push(render_build_logs(logs, show_expanded_logs));
    }

    // Summary line at bottom
    children.push(
        element! {
            View(
                height: 1,
                padding_left: 1,
                padding_right: 1
            ) {
                #(summary_view)
            }
        }
        .into_any(),
    );

    element! {
        View(flex_direction: FlexDirection::Column, height: dynamic_height + 1) {
            #(children)
        }
    }
}

/// Render a single activity (owned version)
fn render_activity_owned(
    activity: ActivityInfo,
    is_selected: bool,
    spinner_frame: usize,
) -> AnyElement<'static> {
    let spinner = SPINNER_FRAMES[spinner_frame];
    let indent = "  ".repeat(activity.depth);

    // Build the activity text
    let activity_text = match &activity.activity_type {
        NixActivityType::Build => {
            let phase = activity.current_phase.as_deref().unwrap_or("building");
            return render_colored_activity(
                indent,
                spinner,
                phase,
                COLOR_ACTIVE,
                &activity.short_name,
                None,
                is_selected,
            );
        }
        NixActivityType::Download => {
            if let (Some(downloaded), Some(total)) =
                (activity.bytes_downloaded, activity.total_bytes)
            {
                let percent = (downloaded as f64 / total as f64 * 100.0) as u8;
                let human_downloaded = downloaded.human_count_bytes().to_string();
                let human_total = total.human_count_bytes().to_string();
                let speed = activity
                    .download_speed
                    .unwrap_or(0.0)
                    .human_throughput_bytes()
                    .to_string();

                // Return early to render with progress bar
                return render_download_with_progress(
                    activity,
                    indent,
                    spinner,
                    downloaded,
                    total,
                    percent,
                    human_downloaded,
                    human_total,
                    speed,
                    is_selected,
                );
            } else {
                return render_colored_activity(
                    indent,
                    spinner,
                    "downloading",
                    COLOR_ACTIVE,
                    &activity.short_name,
                    None,
                    is_selected,
                );
            }
        }
        NixActivityType::Query => {
            let substituter = activity.substituter.as_deref().unwrap_or("cache");
            return render_colored_activity(
                indent,
                spinner,
                "querying",
                COLOR_ACTIVE,
                &activity.short_name,
                Some(&format!("on {}", substituter)),
                is_selected,
            );
        }
        NixActivityType::FetchTree => {
            return render_colored_activity(
                indent,
                spinner,
                "fetching",
                COLOR_ACTIVE,
                &activity.name,
                None,
                is_selected,
            );
        }
        NixActivityType::Evaluating => {
            let suffix = activity
                .evaluation_count
                .as_ref()
                .map(|count| format!("{} files", count));
            return render_colored_activity(
                indent,
                spinner,
                "evaluating",
                COLOR_ACTIVE,
                &activity.name,
                suffix.as_deref(),
                is_selected,
            );
        }
        NixActivityType::Unknown => {
            format!(
                "{}{} {} {}",
                indent, spinner, activity.activity_type, activity.name
            )
        }
    };

    // Add selection highlight
    let color = if is_selected {
        COLOR_INTERACTIVE
    } else {
        Color::Reset
    };

    element! {
        View(height: 1) {
            Text(content: activity_text, color: color)
        }
    }
    .into()
}

/// Build the summary view with colored counts
fn build_summary_view(
    summary: &ActivitySummary,
    has_selection: bool,
    expanded_logs: bool,
    showing_logs: bool,
) -> AnyElement<'static> {
    let mut children = vec![];
    let mut has_content = false;

    // Queries - show if there are any queries (active or done)
    if summary.active_queries > 0 || summary.completed_queries > 0 {
        if has_content {
            children.push(element!(Text(content: "  │  ", color: COLOR_HIERARCHY)).into_any());
        }
        children.push(element!(Text(content: "Queries: ")).into_any());
        if summary.active_queries > 0 {
            children.push(element!(Text(content: format!("{}", summary.active_queries), color: COLOR_ACTIVE, weight: Weight::Bold)).into_any());
            children.push(element!(Text(content: " running")).into_any());
            if summary.completed_queries > 0 {
                children.push(element!(Text(content: ", ")).into_any());
            }
        }
        if summary.completed_queries > 0 {
            children.push(element!(Text(content: format!("{}", summary.completed_queries), color: COLOR_COMPLETED, weight: Weight::Bold)).into_any());
            children.push(element!(Text(content: " done")).into_any());
        }
        has_content = true;
    }

    // Downloads - show if there are any downloads (active or done)
    if summary.active_downloads > 0 || summary.completed_downloads > 0 {
        if has_content {
            children.push(element!(Text(content: "  │  ", color: COLOR_HIERARCHY)).into_any());
        }
        children.push(element!(Text(content: "Downloads: ")).into_any());
        if summary.active_downloads > 0 {
            children.push(element!(Text(content: format!("{}", summary.active_downloads), color: COLOR_ACTIVE, weight: Weight::Bold)).into_any());
            children.push(element!(Text(content: " running")).into_any());
            if summary.completed_downloads > 0 {
                children.push(element!(Text(content: ", ")).into_any());
            }
        }
        if summary.completed_downloads > 0 {
            children.push(element!(Text(content: format!("{}", summary.completed_downloads), color: COLOR_COMPLETED, weight: Weight::Bold)).into_any());
            children.push(element!(Text(content: " done")).into_any());
        }
        has_content = true;
    }

    // Builds - only show if there are any builds (active, completed, or failed)
    if summary.active_builds > 0 || summary.completed_builds > 0 || summary.failed_builds > 0 {
        if has_content {
            children.push(element!(Text(content: "  │  ", color: COLOR_HIERARCHY)).into_any());
        }
        children.push(element!(Text(content: "Builds: ")).into_any());

        let mut first = true;
        if summary.active_builds > 0 {
            children.push(element!(Text(content: format!("{}", summary.active_builds), color: COLOR_ACTIVE, weight: Weight::Bold)).into_any());
            children.push(element!(Text(content: " running")).into_any());
            first = false;
        }
        if summary.completed_builds > 0 {
            if !first {
                children.push(element!(Text(content: ", ")).into_any());
            }
            children.push(element!(Text(content: format!("{}", summary.completed_builds), color: COLOR_COMPLETED, weight: Weight::Bold)).into_any());
            children.push(element!(Text(content: " done")).into_any());
            first = false;
        }
        if summary.failed_builds > 0 {
            if !first {
                children.push(element!(Text(content: ", ")).into_any());
            }
            children.push(element!(Text(content: format!("{}", summary.failed_builds), color: COLOR_FAILED, weight: Weight::Bold)).into_any());
            children.push(element!(Text(content: " failed")).into_any());
        }
        has_content = true;
    }

    // Build help text if needed
    let mut help_children = vec![];
    if has_content {
        if has_selection {
            // Show full navigation when something is selected
            help_children.push(element!(Text(content: "↑↓", color: COLOR_INTERACTIVE)).into_any());
            help_children.push(element!(Text(content: " navigate • ")).into_any());
            help_children.push(element!(Text(content: "e", color: COLOR_INTERACTIVE)).into_any());
            if expanded_logs {
                help_children.push(element!(Text(content: " collapse logs • ")).into_any());
            } else {
                help_children.push(element!(Text(content: " expand logs • ")).into_any());
            }
            help_children.push(element!(Text(content: "Esc", color: COLOR_INTERACTIVE)).into_any());
            if showing_logs {
                help_children.push(element!(Text(content: " hide logs")).into_any());
            } else {
                help_children.push(element!(Text(content: " clear")).into_any());
            }
        } else if summary.active_builds > 0 {
            // Show only navigate hint when builds are running
            help_children.push(element!(Text(content: "↑↓", color: COLOR_INTERACTIVE)).into_any());
            help_children.push(element!(Text(content: " navigate")).into_any());
        }
    }

    // Create layout with stats on left and help on right
    element!(View(flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, width: 100pct) {
        View(flex_direction: FlexDirection::Row) {
            #(children)
        }
        View(flex_direction: FlexDirection::Row, margin_left: 2) {
            #(help_children)
        }
    }).into_any()
}

/// Render download with progress bar
fn render_download_with_progress(
    activity: ActivityInfo,
    indent: String,
    _spinner: &str,
    _downloaded: u64,
    _total: u64,
    percent: u8,
    human_downloaded: String,
    human_total: String,
    speed: String,
    is_selected: bool,
) -> AnyElement<'static> {
    // Create progress bar using box drawing characters
    let bar_width = 20;
    let filled = (bar_width * percent as usize) / 100;
    let empty = bar_width - filled;
    let progress_bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

    let mut children = vec![element!(Text(content: format!("{}", indent))).into_any()];

    // Add hierarchy indicator if indented
    if !indent.is_empty() {
        children.push(element!(Text(content: "└─ ", color: COLOR_HIERARCHY)).into_any());
    }

    children.extend(vec![
        element!(Text(content: "Downloading ", color: COLOR_ACTIVE, weight: Weight::Bold)).into_any(),
        element!(Text(content: format!("{} ", activity.short_name), color: if is_selected { COLOR_INTERACTIVE } else { Color::Reset })).into_any(),
        element!(Text(content: "[")).into_any(),
        element!(Text(content: progress_bar, color: COLOR_ACTIVE)).into_any(),
        element!(Text(content: format!("] {}/{} {}% at {}", human_downloaded, human_total, percent, speed))).into_any(),
    ]);

    element! {
        View(height: 1, flex_direction: FlexDirection::Row) {
            #(children)
        }
    }
    .into()
}

/// Render activity with colored action word
fn render_colored_activity(
    indent: String,
    _spinner: &str,
    action: &str,
    _action_color: Color,
    name: &str,
    suffix: Option<&str>,
    is_selected: bool,
) -> AnyElement<'static> {
    let mut children = vec![element!(Text(content: format!("{}", indent))).into_any()];

    // Add hierarchy indicator if indented
    if !indent.is_empty() {
        children.push(element!(Text(content: "└─ ", color: COLOR_HIERARCHY)).into_any());
    }

    children.extend(vec![
        element!(Text(content: format!("{} ", action.chars().next().unwrap_or_default().to_uppercase().collect::<String>() + &action[1..]), color: COLOR_ACTIVE, weight: Weight::Bold)).into_any(),
        element!(Text(content: name.to_string(), color: if is_selected { COLOR_INTERACTIVE } else { Color::Reset })).into_any(),
    ]);

    if let Some(suffix_text) = suffix {
        children.push(
            element!(Text(content: format!(" {}", suffix_text), color: COLOR_HIERARCHY)).into_any(),
        );
    }

    element! {
        View(height: 1, flex_direction: FlexDirection::Row) {
            #(children)
        }
    }
    .into()
}

/// Render build logs
fn render_build_logs(logs: &VecDeque<String>, expanded: bool) -> AnyElement<'static> {
    let mut log_elements = vec![];

    // Add separator
    log_elements.push(
        element!(
            View(height: 1, padding_left: 1) {
                Text(content: "─".repeat(80), color: COLOR_HIERARCHY)
            }
        )
        .into_any(),
    );

    // Determine how many lines to show
    let lines_to_show = if expanded {
        logs.len().min(50) // Cap at 50 lines even when expanded
    } else {
        10
    };

    // Take last N lines of logs
    let log_lines: Vec<_> = logs.iter().rev().take(lines_to_show).rev().collect();

    // Add log lines
    for line in log_lines {
        log_elements.push(
            element!(
                View(height: 1, padding_left: 2) {
                    Text(content: line.clone(), color: Color::AnsiValue(245))
                }
            )
            .into_any(),
        );
    }

    let total_height = log_elements.len() as u32;

    element! {
        View(height: total_height, flex_direction: FlexDirection::Column) {
            #(log_elements)
        }
    }
    .into_any()
}

/// Format a duration in a human-readable way
pub fn format_duration(duration: Duration) -> String {
    duration.human_duration().to_string()
}
