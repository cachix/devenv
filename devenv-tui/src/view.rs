use crate::{
    components::*,
    model::{Activity, ActivitySummary, ActivityVariant, Model, TaskDisplayStatus},
};
use human_repr::{HumanCount, HumanDuration};
use iocraft::Context;
use iocraft::components::ContextProvider;
use iocraft::prelude::*;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Main view function that creates the UI
pub fn view(model: &Model) -> impl Into<AnyElement<'static>> {
    let active_activities = model.get_active_display_activities();

    let summary = model.calculate_summary();
    let has_selection = model.ui.selected_activity.is_some();
    let spinner_frame = model.ui.spinner_frame;
    let selected_id = model.ui.selected_activity;
    let show_expanded_logs = model.ui.view_options.show_expanded_logs;

    // Check if we have a selected build activity with logs FIRST
    let selected_activity = model.get_selected_activity();
    let build_logs = selected_activity
        .as_ref()
        .filter(|a| matches!(a.variant, ActivityVariant::Build(_)))
        .and_then(|a| model.get_build_logs(a.id));

    // Show all activities (including the selected build activity with inline logs)
    let activities_to_show: Vec<_> = active_activities.iter().collect();

    // Create owned activity elements
    let activity_elements: Vec<_> = activities_to_show
        .iter()
        .map(|display_activity| {
            let activity = &display_activity.activity;
            let is_selected = selected_id.is_some_and(|id| activity.id == id && activity.id != 0);

            // Pass build logs if this is the selected build activity
            let activity_build_logs =
                if is_selected && matches!(activity.variant, ActivityVariant::Build(_)) {
                    build_logs.cloned()
                } else {
                    None
                };

            element! {
                ContextProvider(value: Context::owned(ActivityRenderContext {
                    activity: activity.clone(),
                    depth: display_activity.depth,
                    is_selected,
                    spinner_frame,
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
            has_selection,
            expanded_logs: model.ui.view_options.show_expanded_logs,
            showing_logs: build_logs.is_some(),
        })) {
            SummaryView
        }
    }
    .into_any();

    // Calculate dynamic height based on all activities (including inline logs)
    let mut total_height = 0;
    for display_activity in &activities_to_show {
        total_height += 1; // Base height for activity

        let is_selected = selected_id.is_some_and(|id| {
            display_activity.activity.id == id && display_activity.activity.id != 0
        });

        // Add extra line for downloads with progress
        if matches!(
            display_activity.activity.variant,
            ActivityVariant::Download(_)
        )
            && let ActivityVariant::Download(ref download_data) = display_activity.activity.variant
            {
                if download_data.size_current.is_some() && download_data.size_total.is_some() {
                    total_height += 1; // Extra line for progress bar
                } else if let Some(progress) = &display_activity.activity.progress
                    && progress.total.unwrap_or(0) > 0 {
                        total_height += 1; // Extra line for progress bar
                    }
            }

        // Build activities use early return with custom height - account for it
        if is_selected && matches!(display_activity.activity.variant, ActivityVariant::Build(_))
            && let Some(logs) = build_logs.as_ref() {
                let build_logs_component = BuildLogsComponent::new(Some(logs), show_expanded_logs);
                total_height += build_logs_component.calculate_height(); // Add actual log height
            }
    }
    let min_height = 3; // Minimum height to show at least a few items
    let dynamic_height = total_height.max(min_height) as u32;

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

    // Total height: activities (with inline logs) + summary line + buffer
    let total_height = dynamic_height + 2; // +1 for summary, +1 buffer to prevent overflow

    element! {
        View(flex_direction: FlexDirection::Column, height: total_height, width: 100pct) {
            #(children)
        }
    }
}

/// Context for activity rendering
#[derive(Clone)]
struct ActivityRenderContext {
    activity: Activity,
    depth: usize,
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
        depth,
        is_selected,
        spinner_frame,
        build_logs,
        expanded_logs,
    } = &*ctx;
    let indent = "  ".repeat(*depth);

    // Calculate elapsed time
    let elapsed = Instant::now().duration_since(activity.start_time);
    let elapsed_str = format!("{:.1}s", elapsed.as_secs_f64());

    // Build and return the activity element
    match &activity.variant {
        ActivityVariant::Build(build_data) => {
            // For selected build activities, use custom multi-line rendering
            if *is_selected {
                let phase = build_data.phase.as_deref().unwrap_or("building");
                let prefix = HierarchyPrefixComponent::new(indent, *depth)
                    .with_spinner(*spinner_frame)
                    .render();

                let main_line = ActivityTextComponent::new(
                    "building".to_string(),
                    activity.short_name.clone(),
                    elapsed_str,
                )
                .with_suffix(Some(phase.to_string()))
                .with_selection(*is_selected)
                .render(terminal_width, *depth, prefix);

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
            let phase = build_data.phase.as_deref().unwrap_or("building");
            let prefix = HierarchyPrefixComponent::new(indent, *depth)
                .with_spinner(*spinner_frame)
                .render();

            return ActivityTextComponent::new(
                "building".to_string(),
                activity.short_name.clone(),
                elapsed_str,
            )
            .with_suffix(Some(phase.to_string()))
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);
        }
        ActivityVariant::Task(task_data) => {
            let status_text = match task_data.status {
                TaskDisplayStatus::Pending => "⏳",
                TaskDisplayStatus::Running => "⚡",
                TaskDisplayStatus::Success => "✅",
                TaskDisplayStatus::Failed => "❌",
                TaskDisplayStatus::Skipped => "⏭️",
                TaskDisplayStatus::Cancelled => "🚫",
            };
            let prefix = HierarchyPrefixComponent::new(indent, *depth).render(); // No spinner for tasks, use status icon

            return ActivityTextComponent::new(
                "".to_string(), // No action prefix for tasks
                format!("{} {}", status_text, activity.name),
                elapsed_str,
            )
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);
        }
        ActivityVariant::Download(download_data) => {
            // Check if we have download progress data
            if let (Some(_current), Some(_total)) =
                (download_data.size_current, download_data.size_total)
            {
                return DownloadActivityComponent::new(
                    activity,
                    *depth,
                    *is_selected,
                    *spinner_frame,
                )
                .render(terminal_width);
            } else if let Some(progress) = &activity.progress {
                // Use generic progress if available
                if progress.total.unwrap_or(0) > 0 {
                    return DownloadActivityComponent::new(
                        activity,
                        *depth,
                        *is_selected,
                        *spinner_frame,
                    )
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
                    let prefix = HierarchyPrefixComponent::new(indent, *depth)
                        .with_spinner(*spinner_frame)
                        .render();

                    return ActivityTextComponent::new(
                        "downloading".to_string(),
                        activity.short_name.clone(),
                        elapsed_str,
                    )
                    .with_suffix(from_suffix)
                    .with_selection(*is_selected)
                    .render(terminal_width, *depth, prefix);
                }
            } else {
                // No progress data available
                let from_suffix = download_data
                    .substituter
                    .as_ref()
                    .map(|s| format!("from {}", s));
                let prefix = HierarchyPrefixComponent::new(indent, *depth)
                    .with_spinner(*spinner_frame)
                    .render();

                return ActivityTextComponent::new(
                    "downloading".to_string(),
                    activity.short_name.clone(),
                    elapsed_str,
                )
                .with_suffix(from_suffix)
                .with_selection(*is_selected)
                .render(terminal_width, *depth, prefix);
            }
        }
        ActivityVariant::Query(query_data) => {
            let substituter = query_data.substituter.as_deref().unwrap_or("cache");
            let prefix = HierarchyPrefixComponent::new(indent, *depth)
                .with_spinner(*spinner_frame)
                .render();

            return ActivityTextComponent::new(
                "querying".to_string(),
                activity.short_name.clone(),
                elapsed_str,
            )
            .with_suffix(Some(format!("on {}", substituter)))
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);
        }
        ActivityVariant::FetchTree => {
            let prefix = HierarchyPrefixComponent::new(indent, *depth)
                .with_spinner(*spinner_frame)
                .render();

            return ActivityTextComponent::new(
                "fetching".to_string(),
                activity.name.clone(),
                elapsed_str,
            )
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);
        }
        ActivityVariant::Evaluating => {
            let suffix = activity.detail.clone();
            let prefix = HierarchyPrefixComponent::new(indent, *depth)
                .with_spinner(*spinner_frame)
                .render();

            return ActivityTextComponent::new(
                "evaluating".to_string(),
                activity.name.clone(),
                elapsed_str,
            )
            .with_suffix(suffix)
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);
        }
        ActivityVariant::UserOperation => {
            let prefix = HierarchyPrefixComponent::new(indent, *depth)
                .with_spinner(*spinner_frame)
                .render();

            return ActivityTextComponent::new(
                "".to_string(), // No action prefix for user operations
                activity.name.clone(),
                elapsed_str,
            )
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);
        }
        ActivityVariant::Unknown => {
            let prefix = HierarchyPrefixComponent::new(indent, *depth)
                .with_spinner(*spinner_frame)
                .render();

            return ActivityTextComponent::new(
                "unknown".to_string(),
                activity.name.clone(),
                elapsed_str,
            )
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);
        }
    }
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
            } else if use_symbols {
                help_children.push(element!(Text(content: " ▼ • ")).into_any());
            // expand symbol
            } else if use_short_text {
                help_children.push(element!(Text(content: " expand • ")).into_any());
            } else {
                help_children.push(element!(Text(content: " expand logs • ")).into_any());
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

/// Format a duration in a human-readable way
pub fn format_duration(duration: Duration) -> String {
    duration.human_duration().to_string()
}
