use crate::{
    components::*,
    model::{ActivityInfo, ActivitySummary, Model, TaskDisplayStatus, TaskInfo},
    NixActivityType,
};
use human_repr::{HumanCount, HumanDuration};
use iocraft::components::ContextProvider;
use iocraft::prelude::*;
use iocraft::Context;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Main view function that creates the UI
pub fn view(model: &Model) -> impl Into<AnyElement<'static>> {
    let active_activities = model.get_active_activity_infos();

    let summary = model.calculate_summary();
    let has_selection = model.ui.selected_activity.is_some();
    let spinner_frame = model.ui.spinner_frame;
    let selected_id = model.ui.selected_activity;
    let show_expanded_logs = model.ui.view_options.show_expanded_logs;

    // Check if we have a selected build activity with logs FIRST
    let selected_activity = model.get_selected_activity();
    let build_logs = selected_activity
        .as_ref()
        .filter(|a| matches!(a.activity_type, NixActivityType::Build))
        .and_then(|a| model.get_build_logs(a.id));

    // Show all activities (including the selected build activity with inline logs)
    let activities_to_show: Vec<_> = active_activities.iter().collect();

    // Create owned activity elements
    let activity_elements: Vec<_> = activities_to_show
        .iter()
        .map(|activity| {
            let is_selected = selected_id.map_or(false, |id| activity.activity_id == Some(id));

            // Pass build logs if this is the selected build activity
            let activity_build_logs =
                if is_selected && matches!(activity.activity_type, NixActivityType::Build) {
                    build_logs.cloned()
                } else {
                    None
                };

            element! {
                ContextProvider(value: Context::owned(ActivityRenderContext {
                    activity: (*activity).clone(),
                    is_selected: is_selected,
                    spinner_frame: spinner_frame,
                    build_logs: activity_build_logs,
                    expanded_logs: show_expanded_logs,
                })) {
                    ActivityItem
                }
            }
            .into_any()
        })
        .collect();

    let summary_view = element! {
        ContextProvider(value: Context::owned(SummaryViewContext {
            summary: summary.clone(),
            has_selection: has_selection,
            expanded_logs: model.ui.view_options.show_expanded_logs,
            showing_logs: build_logs.is_some(),
        })) {
            SummaryView
        }
    }
    .into_any();

    // Calculate dynamic height based on all activities (including inline logs)
    let mut total_height = 0;
    for activity in &activities_to_show {
        total_height += 1; // Base height for activity

        let is_selected = selected_id.map_or(false, |id| activity.activity_id == Some(id));

        // Add extra line for downloads with progress
        if matches!(activity.activity_type, NixActivityType::Download) {
            if activity.bytes_downloaded.is_some() && activity.total_bytes.is_some() {
                total_height += 1; // Extra line for progress bar
            } else if let Some(progress) = &activity.generic_progress {
                if progress.expected > 0 {
                    total_height += 1; // Extra line for progress bar
                }
            }
        }

        // Build activities use early return with custom height - account for it
        if is_selected && matches!(activity.activity_type, NixActivityType::Build) {
            if let Some(logs) = build_logs.as_ref() {
                let build_logs_component = BuildLogsComponent::new(Some(logs), show_expanded_logs);
                total_height += build_logs_component.calculate_height(); // Add actual log height
            }
        }
    }
    let min_height = 3; // Minimum height to show at least a few items
    let dynamic_height = total_height.max(min_height) as u32;

    let mut children = vec![];

    // Task summary bar at the top (if we have task info)
    if model.current_task.is_some() || !model.task_hierarchy.is_empty() {
        let task_summary = element! {
            ContextProvider(value: Context::owned(TaskSummaryContext {
                current_task: model.current_task.clone(),
                task_duration: model.get_current_task_duration(),
                task_hierarchy: model.task_hierarchy.clone(),
            })) {
                TaskSummaryBar
            }
        }
        .into_any();

        children.push(
            element! {
                View(
                    height: 1,
                    padding_left: 1,
                    padding_right: 1
                ) {
                    #(task_summary)
                }
            }
            .into_any(),
        );
    }

    // Activity list (with inline logs)
    children.push(
        element! {
            View(flex_grow: 1.0, width: 100pct) {
                View(flex_direction: FlexDirection::Column, width: 100pct) {
                    #(activity_elements)
                }
            }
        }
        .into_any(),
    );

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

    // Total height: task bar + activities (with inline logs) + summary line + buffer
    let task_bar_height = if model.current_task.is_some() || !model.task_hierarchy.is_empty() {
        1
    } else {
        0
    };
    let total_height = dynamic_height + task_bar_height + 2; // +1 for summary, +1 buffer to prevent overflow

    element! {
        View(flex_direction: FlexDirection::Column, height: total_height, width: 100pct) {
            #(children)
        }
    }
}

/// Context for activity rendering
#[derive(Clone)]
struct ActivityRenderContext {
    activity: ActivityInfo,
    is_selected: bool,
    spinner_frame: usize,
    build_logs: Option<VecDeque<String>>,
    expanded_logs: bool,
}

/// Render a single activity (owned version)
#[component]
fn ActivityItem(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let (terminal_width, _) = hooks.use_terminal_size();
    let ctx = hooks.use_context::<ActivityRenderContext>();
    let ActivityRenderContext {
        activity,
        is_selected,
        spinner_frame,
        build_logs,
        expanded_logs,
    } = &*ctx;
    let spinner = SPINNER_FRAMES[*spinner_frame];
    let indent = "  ".repeat(activity.depth);

    // Calculate elapsed time
    let elapsed = Instant::now().duration_since(activity.start_time);
    let elapsed_str = format!("{:.1}s", elapsed.as_secs_f64());

    // Build the activity text
    let activity_text = match &activity.activity_type {
        NixActivityType::Build => {
            // For selected build activities, use custom multi-line rendering
            if *is_selected {
                let phase = activity.current_phase.as_deref().unwrap_or("building");
                let prefix = HierarchyPrefixComponent::new(indent, activity.depth)
                    .with_spinner(*spinner_frame)
                    .render();

                let main_line = ActivityTextComponent::new(
                    "building".to_string(),
                    activity.short_name.clone(),
                    elapsed_str,
                )
                .with_suffix(Some(phase.to_string()))
                .with_selection(*is_selected)
                .render(terminal_width, activity.depth, prefix);

                // Create multi-line element with build logs
                let mut build_elements = vec![main_line];

                // Add build logs using the component
                let build_logs_component =
                    BuildLogsComponent::new(build_logs.as_ref(), *expanded_logs);
                let log_elements = build_logs_component.render();
                build_elements.extend(log_elements);

                // Calculate total height: 1 (main line) + actual log viewport height
                let log_viewport_height = build_logs_component.calculate_height();
                let total_height = (1 + log_viewport_height).min(50) as u32; // Cap total height to prevent overflow
                return element! {
                    View(height: total_height, flex_direction: FlexDirection::Column) {
                        #(build_elements)
                    }
                }
                .into_any();
            }

            // Non-selected build activities use normal rendering
            let phase = activity.current_phase.as_deref().unwrap_or("building");
            let prefix = HierarchyPrefixComponent::new(indent, activity.depth)
                .with_spinner(*spinner_frame)
                .render();

            return ActivityTextComponent::new(
                "building".to_string(),
                activity.short_name.clone(),
                elapsed_str,
            )
            .with_suffix(Some(phase.to_string()))
            .with_selection(*is_selected)
            .render(terminal_width, activity.depth, prefix);
        }
        NixActivityType::Download => {
            // Check if we have byte-level progress
            if let (Some(_downloaded), Some(_total)) =
                (activity.bytes_downloaded, activity.total_bytes)
            {
                // Return early to render with progress bar
                return DownloadActivityComponent::new(
                    activity.clone(),
                    *is_selected,
                    *spinner_frame,
                )
                .render(terminal_width);
            } else if let Some(progress) = &activity.generic_progress {
                // Use generic progress if available
                if progress.expected > 0 {
                    return DownloadActivityComponent::new(
                        activity.clone(),
                        *is_selected,
                        *spinner_frame,
                    )
                    .render(terminal_width);
                } else {
                    // Show generic progress without percentage
                    let from_suffix = activity
                        .substituter
                        .as_ref()
                        .map(|s| format!("from {} [{}]", s, progress.done.human_count_bytes()));
                    let prefix = HierarchyPrefixComponent::new(indent, activity.depth)
                        .with_spinner(*spinner_frame)
                        .render();

                    return ActivityTextComponent::new(
                        "downloading".to_string(),
                        activity.short_name.clone(),
                        elapsed_str,
                    )
                    .with_suffix(from_suffix)
                    .with_selection(*is_selected)
                    .render(terminal_width, activity.depth, prefix);
                }
            } else {
                // No progress data available
                let from_suffix = activity.substituter.as_ref().map(|s| format!("from {}", s));
                let prefix = HierarchyPrefixComponent::new(indent, activity.depth)
                    .with_spinner(*spinner_frame)
                    .render();

                return ActivityTextComponent::new(
                    "downloading".to_string(),
                    activity.short_name.clone(),
                    elapsed_str,
                )
                .with_suffix(from_suffix)
                .with_selection(*is_selected)
                .render(terminal_width, activity.depth, prefix);
            }
        }
        NixActivityType::Query => {
            let substituter = activity.substituter.as_deref().unwrap_or("cache");
            let prefix = HierarchyPrefixComponent::new(indent, activity.depth)
                .with_spinner(*spinner_frame)
                .render();

            return ActivityTextComponent::new(
                "querying".to_string(),
                activity.short_name.clone(),
                elapsed_str,
            )
            .with_suffix(Some(format!("on {}", substituter)))
            .with_selection(*is_selected)
            .render(terminal_width, activity.depth, prefix);
        }
        NixActivityType::FetchTree => {
            let prefix = HierarchyPrefixComponent::new(indent, activity.depth)
                .with_spinner(*spinner_frame)
                .render();

            return ActivityTextComponent::new(
                "fetching".to_string(),
                activity.name.clone(),
                elapsed_str,
            )
            .with_selection(*is_selected)
            .render(terminal_width, activity.depth, prefix);
        }
        NixActivityType::Evaluating => {
            let suffix = activity
                .evaluation_count
                .as_ref()
                .map(|count| format!("{} files", count));
            let prefix = HierarchyPrefixComponent::new(indent, activity.depth)
                .with_spinner(*spinner_frame)
                .render();

            return ActivityTextComponent::new(
                "evaluating".to_string(),
                activity.name.clone(),
                elapsed_str,
            )
            .with_suffix(suffix)
            .with_selection(*is_selected)
            .render(terminal_width, activity.depth, prefix);
        }
        NixActivityType::Unknown => {
            format!(
                "{}{} {} {}",
                indent, spinner, activity.activity_type, activity.name
            )
        }
    };

    // Build the main activity element
    let mut elements = vec![];

    // Add selection highlight
    let color = if *is_selected {
        COLOR_INTERACTIVE
    } else {
        Color::Reset
    };

    // Main activity line
    elements.push(
        element! {
            View(height: 1) {
                Text(content: activity_text, color: color)
            }
        }
        .into_any(),
    );

    let total_height = elements.len() as u32;

    element! {
        View(height: total_height, flex_direction: FlexDirection::Column) {
            #(elements)
        }
    }
    .into()
}

/// Context for summary view rendering
#[derive(Clone)]
struct SummaryViewContext {
    summary: ActivitySummary,
    has_selection: bool,
    expanded_logs: bool,
    showing_logs: bool,
}

/// Summary view component that adapts to terminal width
#[component]
fn SummaryView(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let (terminal_width, _) = hooks.use_terminal_size();
    let ctx = hooks.use_context::<SummaryViewContext>();
    let SummaryViewContext {
        summary,
        has_selection,
        expanded_logs,
        showing_logs,
    } = &*ctx;

    build_summary_view_impl(
        summary,
        *has_selection,
        *expanded_logs,
        *showing_logs,
        terminal_width,
    )
}

/// Build the summary view with colored counts
fn build_summary_view_impl(
    summary: &ActivitySummary,
    has_selection: bool,
    expanded_logs: bool,
    showing_logs: bool,
    terminal_width: u16,
) -> AnyElement<'static> {
    let mut children = vec![];
    let mut has_content = false;

    // Determine display mode based on terminal width
    let use_symbols = terminal_width < 60; // Use unicode symbols for very narrow terminals

    // Builds - only show if there are any builds (active, completed, or failed)
    if summary.active_builds > 0 || summary.completed_builds > 0 || summary.failed_builds > 0 {
        if has_content {
            children.push(element!(View(margin_left: if use_symbols { 1 } else { 2 }, margin_right: if use_symbols { 1 } else { 2 }, flex_shrink: 0.0) {
                Text(content: "│", color: COLOR_HIERARCHY)
            }).into_any());
        }
        let total_builds = summary.active_builds + summary.completed_builds + summary.failed_builds;

        // Format: "2 of 4 builds" or "2/4 builds" - protect numbers from truncation
        if use_symbols {
            children.push(element!(View(margin_right: 1, flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                Text(content: format!("{}", summary.completed_builds), color: COLOR_COMPLETED, weight: Weight::Bold)
                Text(content: format!("/{}", total_builds))
            }).into_any());
        } else {
            children.push(element!(View(margin_right: 1, flex_shrink: 0.0) {
                Text(content: format!("{}", summary.completed_builds), color: COLOR_COMPLETED, weight: Weight::Bold)
            }).into_any());
            children.push(
                element!(View(margin_right: 1, flex_shrink: 0.0) {
                    Text(content: format!("of {}", total_builds))
                })
                .into_any(),
            );
        }

        children.push(
            element!(View(flex_shrink: 0.0) {
                Text(content: "builds")
            })
            .into_any(),
        );
        has_content = true;
    }

    // Downloads - show if there are any downloads (active or done)
    if summary.active_downloads > 0 || summary.completed_downloads > 0 {
        if has_content {
            children.push(element!(View(margin_left: if use_symbols { 1 } else { 2 }, margin_right: if use_symbols { 1 } else { 2 }, flex_shrink: 0.0) {
                Text(content: "│", color: COLOR_HIERARCHY)
            }).into_any());
        }
        let total_downloads = summary.active_downloads + summary.completed_downloads;

        // Format: "3 of 7 downloads" or "3/7 downloads" - protect numbers from truncation
        if use_symbols {
            children.push(element!(View(margin_right: 1, flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                Text(content: format!("{}", summary.completed_downloads), color: COLOR_COMPLETED, weight: Weight::Bold)
                Text(content: format!("/{}", total_downloads))
            }).into_any());
        } else {
            children.push(element!(View(margin_right: 1, flex_shrink: 0.0) {
                Text(content: format!("{}", summary.completed_downloads), color: COLOR_COMPLETED, weight: Weight::Bold)
            }).into_any());
            children.push(
                element!(View(margin_right: 1, flex_shrink: 0.0) {
                    Text(content: format!("of {}", total_downloads))
                })
                .into_any(),
            );
        }

        children.push(
            element!(View(flex_shrink: 0.0) {
                Text(content: "downloads")
            })
            .into_any(),
        );
        has_content = true;
    }

    // Queries - show if there are any queries (active or done)
    if summary.active_queries > 0 || summary.completed_queries > 0 {
        if has_content {
            children.push(element!(View(margin_left: if use_symbols { 1 } else { 2 }, margin_right: if use_symbols { 1 } else { 2 }, flex_shrink: 0.0) {
                Text(content: "│", color: COLOR_HIERARCHY)
            }).into_any());
        }
        let total_queries = summary.active_queries + summary.completed_queries;

        // Format: "5 of 9 queries" or "5/9 queries" - protect numbers from truncation
        if use_symbols {
            children.push(element!(View(margin_right: 1, flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                Text(content: format!("{}", summary.completed_queries), color: COLOR_COMPLETED, weight: Weight::Bold)
                Text(content: format!("/{}", total_queries))
            }).into_any());
        } else {
            children.push(element!(View(margin_right: 1, flex_shrink: 0.0) {
                Text(content: format!("{}", summary.completed_queries), color: COLOR_COMPLETED, weight: Weight::Bold)
            }).into_any());
            children.push(
                element!(View(margin_right: 1, flex_shrink: 0.0) {
                    Text(content: format!("of {}", total_queries))
                })
                .into_any(),
            );
        }

        children.push(
            element!(View(flex_shrink: 0.0) {
                Text(content: "queries")
            })
            .into_any(),
        );
        has_content = true;
    }

    // Build help text if needed - adapt based on terminal width
    let mut help_children = vec![];
    let use_short_text = terminal_width < 100; // Use shorter text for narrow terminals

    if has_content {
        if has_selection {
            // Show full navigation when something is selected
            help_children.push(element!(Text(content: "↑↓", color: COLOR_INTERACTIVE)).into_any());
            if !use_symbols {
                if use_short_text {
                    help_children.push(element!(Text(content: " nav • ")).into_any());
                } else {
                    help_children.push(element!(Text(content: " navigate • ")).into_any());
                }
            } else {
                help_children.push(element!(Text(content: " • ")).into_any());
            }
            help_children.push(element!(Text(content: "e", color: COLOR_INTERACTIVE)).into_any());
            if expanded_logs {
                if use_symbols {
                    help_children.push(element!(Text(content: " ▲ • ")).into_any());
                // collapse symbol
                } else if use_short_text {
                    help_children.push(element!(Text(content: " collapse • ")).into_any());
                } else {
                    help_children.push(element!(Text(content: " collapse logs • ")).into_any());
                }
            } else {
                if use_symbols {
                    help_children.push(element!(Text(content: " ▼ • ")).into_any());
                // expand symbol
                } else if use_short_text {
                    help_children.push(element!(Text(content: " expand • ")).into_any());
                } else {
                    help_children.push(element!(Text(content: " expand logs • ")).into_any());
                }
            }
            help_children.push(element!(Text(content: "Esc", color: COLOR_INTERACTIVE)).into_any());
            if showing_logs {
                if use_symbols {
                    help_children.push(element!(Text(content: " ✕")).into_any());
                // close/hide symbol
                } else if use_short_text {
                    help_children.push(element!(Text(content: " hide")).into_any());
                } else {
                    help_children.push(element!(Text(content: " hide logs")).into_any());
                }
            } else {
                help_children.push(element!(Text(content: " clear")).into_any());
            }
        } else if summary.active_builds > 0 {
            // Show only navigate hint when builds are running
            help_children.push(element!(Text(content: "↑↓", color: COLOR_INTERACTIVE)).into_any());
            if !use_symbols {
                if use_short_text {
                    help_children.push(element!(Text(content: " nav")).into_any());
                } else {
                    help_children.push(element!(Text(content: " navigate")).into_any());
                }
            }
        }
    }

    // Create layout with stats on left and help on right
    element!(View(flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, width: 100pct) {
        View(flex_direction: FlexDirection::Row, flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
            #(children)
        }
        View(flex_direction: FlexDirection::Row, flex_shrink: 0.0, margin_left: if use_symbols { 1 } else { 2 }) {
            #(help_children)
        }
    }).into_any()
}

/// Context for download progress data
#[derive(Clone)]
struct DownloadProgressContext {
    activity: ActivityInfo,
    indent: String,
    percent: u8,
    human_downloaded: String,
    human_total: String,
    speed: String,
    is_selected: bool,
}

/// Download progress component
#[component]
fn DownloadProgress(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let (terminal_width, _) = hooks.use_terminal_size();
    let ctx = hooks.use_context::<DownloadProgressContext>();
    let DownloadProgressContext {
        activity,
        indent,
        percent,
        human_downloaded,
        human_total,
        speed,
        is_selected,
    } = &*ctx;
    // Calculate elapsed time
    let elapsed = Instant::now().duration_since(activity.start_time);
    let elapsed_str = format!("{:.1}s", elapsed.as_secs_f64());

    // Progress calculation is done below with dynamic width

    // Create a two-line display
    let mut elements = vec![];

    // First line: activity name
    let mut line1_children = HierarchyPrefixComponent::new(indent.clone(), activity.depth).render();

    let (shortened_name, _) = calculate_display_info(
        &activity.short_name,
        terminal_width as u32,
        "Downloading",
        activity
            .substituter
            .as_ref()
            .map(|s| format!("from {}", s))
            .as_deref(),
        &elapsed_str,
        activity.depth,
    );
    line1_children.extend(vec![
        element!(View(margin_right: 1) {
            Text(content: "Downloading", color: COLOR_ACTIVE, weight: Weight::Bold)
        }).into_any(),
        element!(View(margin_right: 1) {
            Text(content: shortened_name, color: if *is_selected { COLOR_INTERACTIVE } else { Color::Reset })
        }).into_any(),
    ]);

    if let Some(substituter) = &activity.substituter {
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

    // Second line: progress bar (indented more)
    let progress_indent = format!("{}    ", indent); // Extra indentation for child

    // Calculate space for progress bar - leave room for size info and speed
    let size_info = format!("{} / {} at {}", human_downloaded, human_total, speed);
    let prefix_len = progress_indent.len();
    let size_info_len = size_info.len() + 2; // +2 for spaces

    // Calculate available width for progress bar
    let available_width = (terminal_width as usize)
        .saturating_sub(prefix_len)
        .saturating_sub(size_info_len)
        .saturating_sub(4); // Some padding
    let bar_width = available_width.clamp(10, 100); // Min 10, max 100 chars

    let filled = (bar_width * *percent as usize) / 100;
    let empty = bar_width - filled;

    // Split progress bar into filled and empty parts for coloring
    let filled_bar = "─".repeat(filled);
    let empty_bar = "─".repeat(empty);

    elements.push(
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
    );

    element! {
        View(flex_direction: FlexDirection::Column) {
            #(elements)
        }
    }
    .into_any()
}

/// Format a duration in a human-readable way
pub fn format_duration(duration: Duration) -> String {
    duration.human_duration().to_string()
}

/// Context for task summary bar
#[derive(Clone)]
struct TaskSummaryContext {
    current_task: Option<String>,
    task_duration: Option<std::time::Duration>,
    task_hierarchy: Vec<TaskInfo>,
}

/// Task summary bar component
#[component]
fn TaskSummaryBar(hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let ctx = hooks.use_context::<TaskSummaryContext>();
    let TaskSummaryContext {
        current_task,
        task_duration,
        task_hierarchy,
    } = &*ctx;

    let mut children = vec![];

    // Show current task if available
    if let Some(task_name) = current_task {
        // Task icon
        children.push(
            element!(Text(content: "⚡", color: COLOR_ACTIVE, weight: Weight::Bold)).into_any(),
        );

        // Task name
        children.push(
            element!(View(margin_left: 1, margin_right: 1) {
                Text(content: task_name.clone(), color: COLOR_ACTIVE, weight: Weight::Bold)
            })
            .into_any(),
        );

        // Duration if available
        if let Some(duration) = task_duration {
            let elapsed_str = format!("{}s", duration.as_secs());
            children.push(element!(Text(content: elapsed_str, color: COLOR_HIERARCHY)).into_any());
        }
    }

    // Show task summary counts
    let (running, pending, completed, failed) =
        task_hierarchy
            .iter()
            .fold((0, 0, 0, 0), |(r, p, c, f), task| match task.status {
                TaskDisplayStatus::Running => (r + 1, p, c, f),
                TaskDisplayStatus::Pending => (r, p + 1, c, f),
                TaskDisplayStatus::Success => (r, p, c + 1, f),
                TaskDisplayStatus::Failed => (r, p, c, f + 1),
                TaskDisplayStatus::Skipped => (r, p, c + 1, f), // Count skipped as completed
                TaskDisplayStatus::Cancelled => (r, p, c, f + 1), // Count cancelled as failed
            });

    if running + pending + completed + failed > 0 {
        // Add separator if we have both current task and hierarchy
        if current_task.is_some() {
            children.push(
                element!(View(margin_left: 2, margin_right: 2) {
                    Text(content: "│", color: COLOR_HIERARCHY)
                })
                .into_any(),
            );
        }

        // Running tasks
        if running > 0 {
            children.push(
                element!(View(margin_right: 1) {
                    Text(content: format!("{}", running), color: COLOR_ACTIVE, weight: Weight::Bold)
                })
                .into_any(),
            );
            children.push(
                element!(View(margin_right: 2) {
                    Text(content: "running")
                })
                .into_any(),
            );
        }

        // Completed tasks
        if completed > 0 {
            children.push(
                element!(View(margin_right: 1) {
                    Text(content: format!("{}", completed), color: COLOR_COMPLETED, weight: Weight::Bold)
                })
                .into_any(),
            );
            children.push(
                element!(View(margin_right: 2) {
                    Text(content: "done")
                })
                .into_any(),
            );
        }

        // Failed tasks
        if failed > 0 {
            children.push(
                element!(View(margin_right: 1) {
                    Text(content: format!("{}", failed), color: Color::Red, weight: Weight::Bold)
                })
                .into_any(),
            );
            children.push(
                element!(View(margin_right: 2) {
                    Text(content: "failed")
                })
                .into_any(),
            );
        }

        // Pending tasks
        if pending > 0 {
            children.push(
                element!(View(margin_right: 1) {
                    Text(content: format!("{}", pending), color: COLOR_HIERARCHY, weight: Weight::Bold)
                })
                .into_any(),
            );
            children.push(
                element!(View(margin_right: 2) {
                    Text(content: "pending")
                })
                .into_any(),
            );
        }
    }

    element!(View(flex_direction: FlexDirection::Row, width: 100pct) {
        #(children)
    })
    .into_any()
}
