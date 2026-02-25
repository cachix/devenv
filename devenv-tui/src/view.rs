use crate::{
    components::{LOG_VIEWPORT_FAILED, LOG_VIEWPORT_SHOW_OUTPUT, *},
    model::{
        Activity, ActivityModel, ActivitySummary, ActivityVariant, NixActivityState, RenderContext,
        TaskDisplayStatus, TerminalSize, UiState,
    },
};
use devenv_activity::{ActivityLevel, ProcessStatus};
use human_repr::{HumanCount, HumanDuration};
use iocraft::Context;
use iocraft::components::ContextProvider;
use iocraft::prelude::*;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

/// Main view function that creates the UI
pub fn view(
    model: &ActivityModel,
    ui_state: &UiState,
    render_context: RenderContext,
) -> impl Into<AnyElement<'static>> {
    let active_activities = model.get_display_activities();

    let summary = model.calculate_summary();
    let selected_id = ui_state.selected_activity;
    let terminal_size = ui_state.terminal_size;

    // Check if we have a selected activity with logs/details
    let selected_activity = selected_id.and_then(|id| model.get_activity(id));
    let selected_logs = selected_activity
        .as_ref()
        .and_then(|a| model.get_build_logs(a.id));

    // Show all activities (including the selected activity with inline logs)
    let activities_to_show: Vec<_> = active_activities.iter().collect();

    // Create owned activity elements, including hidden children indicators
    let mut activity_elements: Vec<AnyElement<'static>> = Vec::new();

    for display_activity in activities_to_show.iter() {
        let activity = &display_activity.activity;
        let is_selected = selected_id.is_some_and(|id| activity.id == id && activity.id != 0);

        // Pass logs for activities that should display them:
        // - Tasks with show_output=true or failed: show logs inline
        // - Messages with details: always show details inline
        // - Selected activities: show logs when selected
        let task_failed = matches!(
            (&activity.variant, &activity.state),
            (
                ActivityVariant::Task(_),
                NixActivityState::Completed { success: false, .. }
            )
        );
        let devenv_failed = matches!(
            (&activity.variant, &activity.state),
            (
                ActivityVariant::Devenv,
                NixActivityState::Completed { success: false, .. }
            )
        );
        let show_activity_logs = devenv_failed
            || match &activity.variant {
                ActivityVariant::Task(task_data) => task_data.show_output || task_failed,
                ActivityVariant::Process(_) => true,
                ActivityVariant::Message(msg_data) => msg_data.details.is_some(),
                _ => false,
            };
        let activity_logs = if show_activity_logs {
            model.get_build_logs(activity.id).cloned()
        } else if is_selected {
            selected_logs.cloned()
        } else {
            None
        };

        // Determine completion state
        let (completed, cached) = match &activity.state {
            NixActivityState::Queued | NixActivityState::Active => (None, false),
            NixActivityState::Completed {
                success, cached, ..
            } => (Some(*success), *cached),
        };

        activity_elements.push(
            element! {
                ContextProvider(value: Context::owned(ActivityRenderContext {
                    activity: activity.clone(),
                    depth: display_activity.depth,
                    is_selected,
                    logs: activity_logs,
                    log_line_count: model.get_log_line_count(activity.id),
                    completed,
                    cached,
                    render_context,
                })) {
                    ActivityItem
                }
            }
            .into_any(),
        );
    }

    // Determine if navigation is possible
    let selectable_ids = model.get_selectable_activity_ids();
    let (can_go_up, can_go_down) = if let Some(current_id) = selected_id {
        if let Some(pos) = selectable_ids.iter().position(|&id| id == current_id) {
            (pos > 0, pos + 1 < selectable_ids.len())
        } else {
            (false, !selectable_ids.is_empty())
        }
    } else {
        (false, !selectable_ids.is_empty())
    };

    // Show summary (nav bar) only in normal render context
    let show_summary = render_context == RenderContext::Normal;

    let summary_view = element! {
        ContextProvider(value: Context::owned(SummaryViewContext {
            summary: summary.clone(),
            selected: selected_activity.cloned(),
            showing_logs: selected_logs.is_some(),
            can_go_up,
            can_go_down,
        })) {
            SummaryView
        }
    }
    .into_any();

    let mut children = vec![];

    // Task activities are now included in the regular activity list
    // No separate task bar needed

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

    // Summary line at bottom (only in normal render context)
    if show_summary {
        children.push(
            element! {
                View(
                    height: 1,
                    flex_shrink: 0.0,
                    margin_top: 1,
                    padding_left: 1,
                    padding_right: 1
                ) {
                    #(summary_view)
                }
            }
            .into_any(),
        );
    }

    element! {
        ContextProvider(value: Context::owned(terminal_size)) {
            View(flex_direction: FlexDirection::Column, max_height: terminal_size.height as u32, width: 100pct, overflow: Overflow::Hidden, justify_content: JustifyContent::FlexEnd) {
                #(children)
            }
        }
    }
}

/// Context for activity rendering
#[derive(Clone)]
struct ActivityRenderContext {
    activity: Activity,
    depth: usize,
    is_selected: bool,
    logs: Option<Arc<VecDeque<String>>>,
    /// Total log line count (not affected by buffer rotation)
    log_line_count: usize,
    /// Completion state: None = active, Some(true) = success, Some(false) = failed
    completed: Option<bool>,
    /// Whether this activity's result was cached
    cached: bool,
    /// Whether this is the final render before exit
    render_context: RenderContext,
}

/// Helper to build activity prefix with hierarchy and status indicator.
/// - Top-level (depth == 0): [StatusIndicator]
/// - Nested (depth > 0): [HierarchyPrefix][StatusIndicator]
fn build_activity_prefix(depth: usize, completed: Option<bool>) -> Vec<AnyElement<'static>> {
    let mut prefix = HierarchyPrefixComponent::new(depth).render();

    prefix.push(
        element!(View(margin_right: 1) {
            StatusIndicator(completed: completed, show_spinner: true)
        })
        .into_any(),
    );

    prefix
}

/// Render a single activity (owned version)
#[component]
fn ActivityItem(hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let terminal_width = hooks.use_context::<TerminalSize>().width;
    let ctx = hooks.use_context::<ActivityRenderContext>();
    let ActivityRenderContext {
        activity,
        depth,
        is_selected,
        logs,
        log_line_count,
        completed,
        cached,
        render_context,
    } = &*ctx;

    // Calculate elapsed time - use stored duration for completed activities, skip for queued
    let elapsed_str = match &activity.state {
        NixActivityState::Completed { duration, .. } => format_elapsed_time(*duration, true),
        NixActivityState::Active => format_elapsed_time(activity.start_time.elapsed(), false),
        NixActivityState::Queued => String::new(), // No timer for queued activities
    };

    // Build and return the activity element
    match &activity.variant {
        ActivityVariant::Build(build_data) => {
            let is_completed = completed.is_some();

            // Show line count for completed builds, phase + line count for active builds
            let phase_suffix = if is_completed {
                if *log_line_count > 0 {
                    Some(format!("{} lines", log_line_count))
                } else {
                    None
                }
            } else if *log_line_count > 0 {
                build_data
                    .phase
                    .as_ref()
                    .map(|p| format!("{} ({} lines)", p, log_line_count))
                    .or_else(|| Some(format!("{} lines", log_line_count)))
            } else {
                build_data.phase.clone()
            };

            // For selected build activities, use custom multi-line rendering
            if *is_selected {
                let prefix = build_activity_prefix(*depth, *completed);

                let main_line = ActivityTextComponent::new(
                    "building".to_string(),
                    activity.short_name.clone(),
                    elapsed_str,
                    activity.variant.clone(),
                )
                .with_suffix(phase_suffix.clone())
                .with_completed(is_completed)
                .with_selection(*is_selected)
                .render(terminal_width, *depth, prefix);

                return ExpandedContentComponent::new(logs.as_deref())
                    .with_empty_message("  → no build logs yet (press '^e' to expand)")
                    .render_with_main_line(main_line);
            }

            // Non-selected build activities use normal rendering
            let prefix = build_activity_prefix(*depth, *completed);

            return ActivityTextComponent::new(
                "building".to_string(),
                activity.short_name.clone(),
                elapsed_str,
                activity.variant.clone(),
            )
            .with_suffix(phase_suffix)
            .with_completed(is_completed)
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);
        }
        ActivityVariant::Task(task_data) => {
            // Base status without log line (used as fallback or when no log)
            let base_status = match task_data.status {
                TaskDisplayStatus::Pending => Some("pending".to_string()),
                TaskDisplayStatus::Running if *log_line_count > 0 => {
                    Some(format!("{} lines", log_line_count))
                }
                TaskDisplayStatus::Running => None,
                TaskDisplayStatus::Success if *log_line_count > 0 => {
                    Some(format!("{} lines", log_line_count))
                }
                TaskDisplayStatus::Success => None,
                TaskDisplayStatus::Failed if *log_line_count > 0 => {
                    Some(format!("failed ({} lines)", log_line_count))
                }
                TaskDisplayStatus::Failed => Some("failed".to_string()),
                TaskDisplayStatus::Skipped => Some("skipped".to_string()),
                TaskDisplayStatus::Cancelled => Some("cancelled".to_string()),
            };

            // Append last log line if available (overflow will truncate naturally)
            let status_text =
                if let Some(last_line) = task_data.last_log_line.as_ref().map(|l| l.trim()) {
                    if last_line.is_empty() {
                        base_status
                    } else {
                        match task_data.status {
                            TaskDisplayStatus::Failed => Some(format!("failed → {}", last_line)),
                            _ => match base_status {
                                Some(base) => Some(format!("{} → {}", base, last_line)),
                                None => Some(format!("→ {}", last_line)),
                            },
                        }
                    }
                } else {
                    base_status
                };

            let prefix = build_activity_prefix(*depth, *completed);

            let main_line = ActivityTextComponent::name_only(
                activity.name.clone(),
                elapsed_str,
                activity.variant.clone(),
            )
            .with_suffix(status_text)
            .with_completed(completed.is_some())
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);

            // Show logs inline for tasks with show_output=true or failed tasks
            let task_failed = *completed == Some(false);
            if (task_data.show_output || task_failed) && logs.is_some() {
                let empty_message = if completed.is_some() {
                    "  → no output"
                } else {
                    "  → waiting for output..."
                };
                let mut component = ExpandedContentComponent::new(logs.as_deref())
                    .with_empty_message(empty_message);
                if task_failed {
                    component = component.with_max_lines(LOG_VIEWPORT_FAILED);
                } else if task_data.show_output && !is_selected {
                    component = component.with_max_lines(LOG_VIEWPORT_SHOW_OUTPUT);
                }
                return component.render_with_main_line(main_line);
            }

            return main_line;
        }
        ActivityVariant::Download(download_data) => {
            // Check if we have download progress data
            if let (Some(_current), Some(_total)) =
                (download_data.size_current, download_data.size_total)
            {
                return DownloadActivityComponent::new(activity, *depth, *is_selected)
                    .with_completed(*completed)
                    .with_cached(*cached)
                    .render(terminal_width);
            } else if let Some(progress) = &activity.progress {
                // Use generic progress if available
                if progress.total.unwrap_or(0) > 0 {
                    return DownloadActivityComponent::new(activity, *depth, *is_selected)
                        .with_completed(*completed)
                        .with_cached(*cached)
                        .render(terminal_width);
                } else {
                    // Show generic progress without percentage
                    let from_suffix = download_data.substituter.as_ref().map(|s| {
                        format!(
                            "from {} [{}]",
                            s,
                            progress.current.unwrap_or(0).human_count_bytes()
                        )
                    });
                    let prefix = build_activity_prefix(*depth, *completed);

                    return ActivityTextComponent::new(
                        "downloading".to_string(),
                        activity.short_name.clone(),
                        elapsed_str,
                        activity.variant.clone(),
                    )
                    .with_suffix(from_suffix)
                    .with_completed(completed.is_some())
                    .with_selection(*is_selected)
                    .render(terminal_width, *depth, prefix);
                }
            } else {
                // No progress data available
                let from_suffix = download_data
                    .substituter
                    .as_ref()
                    .map(|s| format!("from {}", s));
                let prefix = build_activity_prefix(*depth, *completed);

                return ActivityTextComponent::new(
                    "downloading".to_string(),
                    activity.short_name.clone(),
                    elapsed_str,
                    activity.variant.clone(),
                )
                .with_suffix(from_suffix)
                .with_completed(completed.is_some())
                .with_selection(*is_selected)
                .render(terminal_width, *depth, prefix);
            }
        }
        ActivityVariant::Copy => {
            let prefix = build_activity_prefix(*depth, *completed);

            return ActivityTextComponent::new(
                "copying".to_string(),
                activity.short_name.clone(),
                elapsed_str,
                activity.variant.clone(),
            )
            .with_suffix(Some("to the store".to_string()))
            .with_completed(completed.is_some())
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);
        }
        ActivityVariant::Query(query_data) => {
            let suffix = query_data
                .substituter
                .as_ref()
                .map(|s| format!("from {}", s));
            let prefix = build_activity_prefix(*depth, *completed);

            return ActivityTextComponent::new(
                "querying".to_string(),
                activity.short_name.clone(),
                elapsed_str,
                activity.variant.clone(),
            )
            .with_suffix(suffix)
            .with_completed(completed.is_some())
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);
        }
        ActivityVariant::FetchTree => {
            let prefix = build_activity_prefix(*depth, *completed);

            return ActivityTextComponent::new(
                "fetching".to_string(),
                activity.name.clone(),
                elapsed_str,
                activity.variant.clone(),
            )
            .with_completed(completed.is_some())
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);
        }
        ActivityVariant::Evaluating(eval_data) => {
            // Show cached status or file count as suffix
            let suffix = if *cached {
                Some("cached".to_string())
            } else if eval_data.files_evaluated > 0 {
                Some(format!("{} files", eval_data.files_evaluated))
            } else {
                activity.detail.clone()
            };

            // For selected evaluation activities, show expandable file list
            if *is_selected && logs.is_some() {
                let prefix = build_activity_prefix(*depth, *completed);

                let main_line = ActivityTextComponent::name_only(
                    activity.name.clone(),
                    elapsed_str,
                    activity.variant.clone(),
                )
                .with_suffix(suffix)
                .with_completed(completed.is_some())
                .with_selection(*is_selected)
                .render(terminal_width, *depth, prefix);

                return ExpandedContentComponent::new(logs.as_deref())
                    .with_empty_message("  → no files evaluated yet (press '^e' to expand)")
                    .render_with_main_line(main_line);
            }

            let prefix = build_activity_prefix(*depth, *completed);

            return ActivityTextComponent::name_only(
                activity.name.clone(),
                elapsed_str,
                activity.variant.clone(),
            )
            .with_suffix(suffix)
            .with_completed(completed.is_some())
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);
        }
        ActivityVariant::UserOperation => {
            let prefix = build_activity_prefix(*depth, *completed);

            return ActivityTextComponent::name_only(
                activity.name.clone(),
                elapsed_str,
                activity.variant.clone(),
            )
            .with_completed(completed.is_some())
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);
        }
        ActivityVariant::Devenv => {
            let prefix = build_activity_prefix(*depth, *completed);

            // Show line count as suffix when active or failed with logs
            let suffix = if *completed == Some(true) {
                // Success - no suffix needed
                None
            } else if let Some(ref progress) = activity.progress {
                // Show progress with optional detail
                let progress_text = match (progress.current, progress.total) {
                    (Some(current), Some(total)) if total > 0 => {
                        format!("{}/{}", current, total)
                    }
                    _ => String::new(),
                };
                match (&activity.detail, progress_text.is_empty()) {
                    (Some(detail), false) => Some(format!("{} → {}", progress_text, detail)),
                    (Some(detail), true) => Some(format!("→ {}", detail)),
                    (None, false) => Some(progress_text),
                    (None, true) => None,
                }
            } else if let Some(ref detail) = activity.detail {
                Some(format!("→ {}", detail))
            } else if *log_line_count > 0 {
                // In progress or failed with logs - show line count
                Some(format!("{} lines", log_line_count))
            } else {
                None
            };

            let main_line = ActivityTextComponent::name_only(
                activity.name.clone(),
                elapsed_str,
                activity.variant.clone(),
            )
            .with_suffix(suffix)
            .with_completed(completed.is_some())
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);

            // Show logs when selected or when failed
            let failed = *completed == Some(false);
            if (failed || *is_selected) && logs.is_some() {
                let mut component = ExpandedContentComponent::new(logs.as_deref())
                    .with_empty_message("  → no output yet");
                if failed {
                    component = component.with_max_lines(LOG_VIEWPORT_FAILED);
                }
                return component.render_with_main_line(main_line);
            }

            return main_line;
        }
        ActivityVariant::Process(process_data) => {
            // Build status text with optional ports
            let status_str = match process_data.status {
                ProcessStatus::Running => "running",
                ProcessStatus::Ready => "ready",
                ProcessStatus::Restarting => "restarting",
                ProcessStatus::Stopped => "stopped",
            };

            // Format ports: extract just the port numbers for brevity
            let ports_suffix = if !process_data.ports.is_empty() {
                let port_list: Vec<String> = process_data
                    .ports
                    .iter()
                    .filter_map(|spec| {
                        // spec is like "http:8080" or just "8080"
                        // Extract the port number (last part after colon)
                        spec.rsplit(':').next().map(|p| format!(":{}", p))
                    })
                    .collect();
                if port_list.is_empty() {
                    String::new()
                } else {
                    format!(" {}", port_list.join(", "))
                }
            } else {
                String::new()
            };

            let status_text = Some(format!("{}{}", status_str, ports_suffix));

            let prefix = build_activity_prefix(*depth, *completed);

            let main_line = ActivityTextComponent::new(
                "".to_string(),
                activity.name.clone(),
                elapsed_str,
                activity.variant.clone(),
            )
            .with_suffix(status_text)
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);

            // Show logs: always show LOG_VIEWPORT_SHOW_OUTPUT lines,
            // expand when selected, show more when failed
            let process_failed = *completed == Some(false);
            if logs.is_some() {
                let mut component = ExpandedContentComponent::new(logs.as_deref())
                    .with_depth(*depth)
                    .with_empty_message("→ no output yet (press 'e' to expand)");
                if process_failed && *render_context == RenderContext::Final {
                    component = component.with_max_lines(LOG_VIEWPORT_FAILED);
                } else if !is_selected {
                    component = component.with_max_lines(LOG_VIEWPORT_SHOW_OUTPUT);
                }

                let mut elements = vec![main_line];
                let log_elements = component.render();
                elements.extend(log_elements);

                let log_viewport_height = component.calculate_height();
                let total_height = (1 + log_viewport_height).min(50) as u32;
                return element! {
                    View(height: total_height, flex_direction: FlexDirection::Column) {
                        #(elements)
                    }
                }
                .into_any();
            }

            return main_line;
        }
        ActivityVariant::Message(msg_data) => {
            // Determine icon and color based on message level
            // Following CLI conventions: errors get ✗, others get • (dot)
            let (icon, icon_color, text_color) = match msg_data.level {
                ActivityLevel::Error => ("✗", COLOR_FAILED, COLOR_FAILED),
                ActivityLevel::Warn => ("•", Color::AnsiValue(214), Color::AnsiValue(214)), // Yellow
                ActivityLevel::Info => ("•", COLOR_INFO, Color::Reset), // Blue dot
                _ => ("•", COLOR_HIERARCHY, Color::Reset),              // Gray dot for debug/trace
            };

            // Colors for selected vs unselected rows
            let (selected_text_color, bg_color) = if *is_selected {
                (Color::AnsiValue(232), Some(Color::AnsiValue(250))) // Near-black on light gray
            } else {
                (text_color, None)
            };

            // Build prefix string for indentation
            let prefix_str = if *depth > 0 {
                let spinner_offset = 2;
                let nesting_indent = "  ".repeat(*depth - 1);
                format!("{}{}", " ".repeat(spinner_offset), nesting_indent)
            } else {
                String::new()
            };

            // For errors, always show full message including details
            // Show trace first, then the error summary at the bottom
            let has_details = msg_data.details.is_some();
            if has_details && logs.is_some() {
                let mut all_lines: Vec<AnyElement<'static>> = vec![];

                // First add detail/trace lines (collapsed preview, press 'e' to expand)
                if let Some(detail_lines) = logs.as_deref() {
                    let visible_lines: Vec<_> = detail_lines
                        .iter()
                        .rev()
                        .take(LOG_VIEWPORT_FAILED)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();

                    for line in visible_lines {
                        all_lines.push(
                            element! {
                                View(height: 1, flex_direction: FlexDirection::Row, padding_right: 1) {
                                    View(flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                                        Text(content: prefix_str.clone())
                                        View(margin_right: 1) {
                                            Text(content: " ")
                                        }
                                    }
                                    View(flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                                        Text(content: line.clone(), color: COLOR_HIERARCHY)
                                    }
                                }
                            }
                            .into_any(),
                        );
                    }
                }

                // Last line: icon + error summary (with inverse highlight if selected)
                if let Some(bg) = bg_color {
                    all_lines.push(
                        element! {
                            View(height: 1, flex_direction: FlexDirection::Row, padding_right: 1, background_color: bg) {
                                View(flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                                    Text(content: prefix_str.clone())
                                    View(margin_right: 1) {
                                        Text(content: icon, color: icon_color)
                                    }
                                }
                                View(flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                                    Text(content: activity.name.clone(), color: selected_text_color)
                                }
                            }
                        }
                        .into_any(),
                    );
                } else {
                    all_lines.push(
                        element! {
                            View(height: 1, flex_direction: FlexDirection::Row, padding_right: 1) {
                                View(flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                                    Text(content: prefix_str.clone())
                                    View(margin_right: 1) {
                                        Text(content: icon, color: icon_color)
                                    }
                                }
                                View(flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                                    Text(content: activity.name.clone(), color: selected_text_color)
                                }
                            }
                        }
                        .into_any(),
                    );
                }

                let total_height = all_lines.len() as u32;
                return element! {
                    View(height: total_height, flex_direction: FlexDirection::Column) {
                        #(all_lines)
                    }
                }
                .into_any();
            }

            // Simple single-line message (no details)
            if let Some(bg) = bg_color {
                return element! {
                    View(height: 1, flex_direction: FlexDirection::Row, padding_right: 1, background_color: bg) {
                        View(flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                            Text(content: prefix_str)
                            View(margin_right: 1) {
                                Text(content: icon, color: icon_color)
                            }
                        }
                        View(flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                            Text(content: activity.name.clone(), color: selected_text_color)
                        }
                    }
                }
                .into_any();
            } else {
                return element! {
                    View(height: 1, flex_direction: FlexDirection::Row, padding_right: 1) {
                        View(flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                            Text(content: prefix_str)
                            View(margin_right: 1) {
                                Text(content: icon, color: icon_color)
                            }
                        }
                        View(flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                            Text(content: activity.name.clone(), color: selected_text_color)
                        }
                    }
                }
                .into_any();
            }
        }
        ActivityVariant::Unknown => {
            let prefix = build_activity_prefix(*depth, *completed);

            return ActivityTextComponent::new(
                "unknown".to_string(),
                activity.name.clone(),
                elapsed_str,
                activity.variant.clone(),
            )
            .with_completed(completed.is_some())
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);
        }
    }
}

/// Context for summary view rendering
#[derive(Clone)]
struct SummaryViewContext {
    summary: ActivitySummary,
    selected: Option<Activity>,
    showing_logs: bool,
    can_go_up: bool,
    can_go_down: bool,
}

/// Summary view component that adapts to terminal width
#[component]
fn SummaryView(hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let terminal_width = hooks.use_context::<TerminalSize>().width;
    let ctx = hooks.use_context::<SummaryViewContext>();
    let SummaryViewContext {
        summary,
        selected,
        showing_logs,
        can_go_up,
        can_go_down,
    } = &*ctx;

    build_summary_view_impl(
        summary,
        selected.as_ref(),
        *showing_logs,
        *can_go_up,
        *can_go_down,
        terminal_width,
    )
}

/// Build the summary view with colored counts
fn build_summary_view_impl(
    summary: &ActivitySummary,
    selected: Option<&Activity>,
    showing_logs: bool,
    can_go_up: bool,
    can_go_down: bool,
    terminal_width: u16,
) -> AnyElement<'static> {
    let mut children = vec![];
    let mut has_content = false;

    // Derive capabilities from selected activity
    let has_selection = selected.is_some();
    let is_process =
        matches!(selected, Some(a) if matches!(a.variant, ActivityVariant::Process(_)));

    // Determine display mode based on terminal width
    let use_symbols = terminal_width < 60; // Use unicode symbols for very narrow terminals

    // Builds - only show if there are any builds (active, completed, or failed)
    if summary.active_builds > 0 || summary.completed_builds > 0 || summary.failed_builds > 0 {
        if has_content {
            children.push(element!(View(margin_left: if use_symbols { 1 } else { 2 }, margin_right: if use_symbols { 1 } else { 2 }, flex_shrink: 0.0) {
                Text(content: "│", color: COLOR_HIERARCHY)
            }).into_any());
        }
        // Use expected count from SetExpected events if available, otherwise fall back to observed total
        let observed_total =
            summary.active_builds + summary.completed_builds + summary.failed_builds;
        let total_builds = summary
            .expected_builds
            .map(|e| e as usize)
            .unwrap_or(observed_total)
            .max(observed_total);

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
        // Use expected count from SetExpected events if available, otherwise fall back to observed total
        let observed_total = summary.active_downloads + summary.completed_downloads;
        let total_downloads = summary
            .expected_downloads
            .map(|e| e as usize)
            .unwrap_or(observed_total)
            .max(observed_total);

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

    // Tasks - show if there are any tasks
    if summary.running_tasks > 0 || summary.completed_tasks > 0 || summary.failed_tasks > 0 {
        if has_content {
            children.push(element!(View(margin_left: if use_symbols { 1 } else { 2 }, margin_right: if use_symbols { 1 } else { 2 }, flex_shrink: 0.0) {
                Text(content: "│", color: COLOR_HIERARCHY)
            }).into_any());
        }
        let total_tasks = summary.running_tasks + summary.completed_tasks + summary.failed_tasks;

        // Format: "3 of 5 tasks" or "3/5 tasks"
        if use_symbols {
            children.push(element!(View(margin_right: 1, flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                Text(content: format!("{}", summary.completed_tasks), color: COLOR_COMPLETED, weight: Weight::Bold)
                Text(content: format!("/{}", total_tasks))
            }).into_any());
        } else {
            children.push(element!(View(margin_right: 1, flex_shrink: 0.0) {
                Text(content: format!("{}", summary.completed_tasks), color: COLOR_COMPLETED, weight: Weight::Bold)
            }).into_any());
            children.push(
                element!(View(margin_right: 1, flex_shrink: 0.0) {
                    Text(content: format!("of {}", total_tasks))
                })
                .into_any(),
            );
        }

        children.push(
            element!(View(flex_shrink: 0.0) {
                Text(content: "tasks")
            })
            .into_any(),
        );
        has_content = true;
    }

    // Processes - show if there are any running
    if summary.running_processes > 0 {
        if has_content {
            children.push(element!(View(margin_left: if use_symbols { 1 } else { 2 }, margin_right: if use_symbols { 1 } else { 2 }, flex_shrink: 0.0) {
                Text(content: "│", color: COLOR_HIERARCHY)
            }).into_any());
        }

        children.push(element!(View(margin_right: 1, flex_shrink: 0.0) {
            Text(content: format!("{}", summary.running_processes), color: COLOR_COMPLETED, weight: Weight::Bold)
        }).into_any());

        children.push(
            element!(View(flex_shrink: 0.0) {
                Text(content: if summary.running_processes == 1 { "process" } else { "processes" })
            })
            .into_any(),
        );
    }

    // Build help text - always show, adapt based on terminal width
    let mut help_children = vec![];
    let use_short_text = terminal_width < 100; // Use shorter text for narrow terminals

    let up_arrow_color = if can_go_up {
        COLOR_INTERACTIVE
    } else {
        COLOR_HIERARCHY
    };
    let down_arrow_color = if can_go_down {
        COLOR_INTERACTIVE
    } else {
        COLOR_HIERARCHY
    };

    if has_selection {
        // Show full navigation when something is selected
        help_children.push(element!(Text(content: "↑", color: up_arrow_color)).into_any());
        help_children.push(element!(Text(content: "↓", color: down_arrow_color)).into_any());
        if !use_symbols {
            if use_short_text {
                help_children.push(element!(Text(content: " nav • ")).into_any());
            } else {
                help_children.push(element!(Text(content: " navigate • ")).into_any());
            }
        } else {
            help_children.push(element!(Text(content: " • ")).into_any());
        }
        help_children.push(element!(Text(content: "^e", color: COLOR_INTERACTIVE)).into_any());
        if use_symbols {
            help_children.push(element!(Text(content: " ▼ • ")).into_any());
        } else if use_short_text {
            help_children.push(element!(Text(content: " expand • ")).into_any());
        } else {
            help_children.push(element!(Text(content: " expand logs • ")).into_any());
        }
        if is_process {
            help_children.push(element!(Text(content: "^r", color: COLOR_INTERACTIVE)).into_any());
            if use_short_text {
                help_children.push(element!(Text(content: " restart • ")).into_any());
            } else {
                help_children.push(element!(Text(content: " restart process • ")).into_any());
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
    } else {
        // Show navigate hint only when no selection (^e requires selection)
        help_children.push(element!(Text(content: "↑", color: up_arrow_color)).into_any());
        help_children.push(element!(Text(content: "↓", color: down_arrow_color)).into_any());
        if !use_symbols {
            if use_short_text {
                help_children.push(element!(Text(content: " nav")).into_any());
            } else {
                help_children.push(element!(Text(content: " navigate")).into_any());
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

/// Format a duration in a human-readable way
pub fn format_duration(duration: Duration) -> String {
    if cfg!(feature = "deterministic-tui") {
        return "[TIME]".to_string();
    }
    duration.human_duration().to_string()
}
