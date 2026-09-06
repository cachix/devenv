use crate::{
    components::{LOG_VIEWPORT_FAILED, LOG_VIEWPORT_SHOW_OUTPUT, *},
    config::StatuslinePosition,
    model::{
        Activity, ActivityModel, ActivitySummary, ActivityVariant, DisplayActivity,
        NixActivityState, RenderContext, TaskDisplayStatus, TerminalSize, UiState,
    },
    statusline::{
        RenderedSegment, StatuslineData, StatuslineMode, action_key_hints,
        interrupt_prompt_key_hints, render_statusline, separator_style,
    },
};
use devenv_activity::{ActivityLevel, ProcessStatus};
use human_repr::{HumanCount, HumanDuration};
use iocraft::Context;
use iocraft::components::ContextProvider;
use iocraft::hooks::UseComponentRect;
use iocraft::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

/// Height reserved for the bottom summary bar (1 line content + 1 line margin_top).
pub const SUMMARY_BAR_HEIGHT: u16 = 2;

pub(crate) fn summary_bar_height(ui_state: &UiState) -> u16 {
    if ui_state.preferences.statusline.enabled
        || ui_state.interrupt_prompt_active()
        || ui_state.process_search.is_some()
    {
        SUMMARY_BAR_HEIGHT
    } else {
        0
    }
}

pub(crate) fn available_activity_height(ui_state: &UiState) -> usize {
    usize::from(
        ui_state
            .terminal_size
            .height
            .saturating_sub(summary_bar_height(ui_state)),
    )
}

/// Map from activity_id to rendered height in lines.
pub type ActivityHeights = Ref<HashMap<u64, i32>>;

/// Scroll state and the display list the app already computed for this frame.
///
/// Passing `display_activities` through avoids re-walking the activity tree
/// inside `view()`; the app needs the same list to measure heights before
/// rendering.
pub struct ScrollState {
    pub handle: Option<Ref<ScrollViewHandle>>,
    pub display_activities: Vec<DisplayActivity>,
    pub process_previews_fit: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct TreePosition {
    ancestor_continuations: Vec<bool>,
    is_last_sibling: bool,
}

fn tree_positions(activities: &[DisplayActivity]) -> Vec<TreePosition> {
    let depths: Vec<_> = activities.iter().map(|activity| activity.depth).collect();
    tree_positions_for_depths(&depths)
}

fn tree_positions_for_depths(depths: &[usize]) -> Vec<TreePosition> {
    let mut positions = vec![TreePosition::default(); depths.len()];
    let mut last_at_depth: Vec<Option<usize>> = Vec::new();

    for (index, depth) in depths.iter().copied().enumerate() {
        last_at_depth.truncate(depth + 1);
        if last_at_depth.len() <= depth {
            last_at_depth.resize(depth + 1, None);
        }
        if let Some(previous) = last_at_depth[depth].replace(index) {
            positions[previous].is_last_sibling = false;
        }
        positions[index].is_last_sibling = true;
    }

    let last_siblings: Vec<_> = positions
        .iter()
        .map(|position| position.is_last_sibling)
        .collect();
    let mut ancestors: Vec<usize> = Vec::new();

    for (index, depth) in depths.iter().copied().enumerate() {
        ancestors.truncate(depth);
        positions[index].ancestor_continuations = ancestors
            .iter()
            .skip(1)
            .map(|ancestor| !last_siblings[*ancestor])
            .collect();
        ancestors.push(index);
    }

    positions
}

pub(crate) fn process_previews_fit(
    model: &ActivityModel,
    activities: &[DisplayActivity],
    ui_state: &UiState,
) -> bool {
    let terminal_size = ui_state.terminal_size;
    let activity_rows: usize = activities
        .iter()
        .map(|display| non_process_activity_height(model, display, terminal_size))
        .sum();
    let preview_rows: usize = activities
        .iter()
        .filter(|display| matches!(display.activity.variant, ActivityVariant::Process(_)))
        .filter_map(|display| {
            model
                .get_build_logs(display.activity.id)
                .filter(|logs| !logs.is_empty())
                .map(|logs| {
                    ExpandedContentComponent::new(Some(logs.as_ref()))
                        .with_terminal_width(terminal_size.width)
                        .with_border_indent(display.depth * 2)
                        .with_max_lines(model.config().log_viewport_collapsed)
                        .visual_height()
                })
        })
        .sum();
    let available_rows = available_activity_height(ui_state);

    preview_rows > 0 && activity_rows + preview_rows <= available_rows
}

/// Height of an activity before automatic process log previews are added.
///
/// Process rows are always one line at this stage. For other activities, this
/// mirrors the multi-line layouts in `ActivityItem` so the preview preflight
/// does not enable previews that would immediately require scrolling.
fn non_process_activity_height(
    model: &ActivityModel,
    display: &DisplayActivity,
    terminal_size: TerminalSize,
) -> usize {
    let activity = &display.activity;
    let expanded_content_height = |max_lines| {
        model
            .get_build_logs(activity.id)
            .map(|logs| {
                1 + ExpandedContentComponent::new(Some(logs.as_ref()))
                    .with_terminal_width(terminal_size.width)
                    .with_max_lines(max_lines)
                    .visual_height()
            })
            .unwrap_or(1)
    };

    match &activity.variant {
        ActivityVariant::Process(_) => 1,
        ActivityVariant::Task(task_data) => {
            let failed = matches!(
                &activity.state,
                NixActivityState::Completed { success: false, .. }
            );
            if failed {
                expanded_content_height(LOG_VIEWPORT_FAILED)
            } else if task_data.show_output {
                expanded_content_height(LOG_VIEWPORT_SHOW_OUTPUT)
            } else {
                1
            }
        }
        ActivityVariant::Download(download_data) => {
            if download_data.size_current.is_some() && download_data.size_total.is_some()
                || activity
                    .progress
                    .as_ref()
                    .is_some_and(|progress| progress.total.unwrap_or(0) > 0)
            {
                2
            } else {
                1
            }
        }
        ActivityVariant::Evaluating(_) | ActivityVariant::Devenv
            if matches!(
                &activity.state,
                NixActivityState::Completed { success: false, .. }
            ) =>
        {
            expanded_content_height(LOG_VIEWPORT_FAILED)
        }
        ActivityVariant::Message(message_data) if message_data.details.is_some() => model
            .get_build_logs(activity.id)
            .map(|logs| 1 + logs.len().min(LOG_VIEWPORT_FAILED))
            .unwrap_or(1),
        _ => 1,
    }
}

pub(crate) fn activity_shows_inline_logs(
    model: &ActivityModel,
    ui_state: &UiState,
    activity_id: u64,
    process_previews_fit: bool,
) -> bool {
    if ui_state.inline_logs_activity == Some(activity_id) {
        return true;
    }
    if ui_state.inline_logs_activity.is_some()
        || ui_state.process_previews_hidden
        || !process_previews_fit
    {
        return false;
    }
    model.get_activity(activity_id).is_some_and(|activity| {
        matches!(activity.variant, ActivityVariant::Process(_))
            && model
                .get_build_logs(activity_id)
                .is_some_and(|logs| !logs.is_empty())
    })
}

/// Main view function that creates the UI
pub fn view(
    model: &ActivityModel,
    ui_state: &UiState,
    render_context: RenderContext,
    scroll: Option<ScrollState>,
    shutting_down: bool,
) -> impl Into<AnyElement<'static>> {
    let live_layout = scroll.is_some();
    let (scroll_handle, active_activities, process_previews_fit) = match scroll {
        Some(s) => (s.handle, s.display_activities, s.process_previews_fit),
        None => {
            let activities = model.get_display_activities(ui_state);
            let previews_fit = process_previews_fit(model, &activities, ui_state);
            (None, activities, previews_fit)
        }
    };

    let summary = model.calculate_summary();
    let terminal_size = ui_state.terminal_size;
    let tree_positions = tree_positions(&active_activities);

    let selected_id = ui_state
        .selected_activity
        .filter(|id| active_activities.iter().any(|da| da.activity.id == *id));
    let selected_activity = selected_id.and_then(|id| model.get_activity(id));
    let activities_to_show: Vec<_> = active_activities.iter().collect();
    let process_search_match_ids = ui_state.process_search.as_ref().map(|search| {
        model
            .get_matching_process_activity_ids_from_display(&active_activities, &search.query)
            .into_iter()
            .collect::<HashSet<_>>()
    });

    // Create owned activity elements, including hidden children indicators
    let mut activity_elements: Vec<AnyElement<'static>> = Vec::new();

    for (display_activity, tree_position) in activities_to_show.iter().zip(tree_positions) {
        let activity = &display_activity.activity;
        let is_selected = selected_id.is_some_and(|id| activity.id == id && activity.id != 0);
        let is_dimmed = activity_is_dimmed(activity, process_search_match_ids.as_ref());
        let show_inline_logs =
            activity_shows_inline_logs(model, ui_state, activity.id, process_previews_fit);
        let disclosure = model
            .collapsible_descendant_count(activity.id, ui_state)
            .map(|descendant_count| ActivityDisclosure {
                collapsed: !ui_state.expanded_activities.contains(&activity.id),
                descendant_count,
            });

        // Pass logs for activities that should display them:
        // - Tasks with show_output=true or failed: show logs inline
        // - Messages with details: always show details inline
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
        let evaluate_failed = matches!(
            (&activity.variant, &activity.state),
            (
                ActivityVariant::Evaluating(_),
                NixActivityState::Completed { success: false, .. }
            )
        );
        let show_activity_logs = devenv_failed
            || evaluate_failed
            || match &activity.variant {
                ActivityVariant::Task(task_data) => task_data.show_output || task_failed,
                ActivityVariant::Process(_) => true,
                ActivityVariant::Message(msg_data) => msg_data.details.is_some(),
                _ => false,
            };
        let activity_logs = if show_activity_logs || show_inline_logs {
            model.get_build_logs(activity.id).cloned()
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

        let hidden_children_count = if matches!(activity.variant, ActivityVariant::Devenv) {
            model.count_hidden_process_children(activity.id, ui_state)
        } else {
            0
        };

        activity_elements.push(
            element! {
                ContextProvider(value: Context::owned(ActivityRenderContext {
                    activity: activity.clone(),
                    depth: display_activity.depth,
                    tree_position,
                    disclosure,
                    is_selected,
                    is_dimmed,
                    show_inline_logs,
                    logs: activity_logs,
                    log_line_count: model.get_log_line_count(activity.id),
                    log_preview_lines: model.config().log_viewport_collapsed,
                    completed,
                    cached,
                    render_context,
                    shutting_down,
                    hidden_children_count,
                })) {
                    ActivityItem
                }
            }
            .into_any(),
        );
    }

    // Determine if navigation is possible
    let selectable_ids =
        model.get_selectable_activity_ids_from_display(&active_activities, ui_state);
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
    let show_summary = render_context == RenderContext::Normal && summary_bar_height(ui_state) > 0;

    let summary_view = element! {
        ContextProvider(value: Context::owned(SummaryViewContext {
            preferences: ui_state.preferences.clone(),
            keymap: ui_state.keymap().clone(),
            run_context: ui_state.run_context.clone(),
            pending_key: ui_state.pending_key.clone(),
            summary: summary.clone(),
            selected: selected_activity.cloned(),
            showing_logs: selected_id.is_some_and(|id| {
                activity_shows_inline_logs(model, ui_state, id, process_previews_fit)
            }),
            can_go_up,
            can_go_down,
            interrupt_prompt_active: ui_state.interrupt_prompt_active(),
            interrupt_prompt_attached: ui_state.interrupt_prompt_attached(),
            hide_stopped_processes: ui_state.hide_stopped_processes,
            process_search: ui_state
                .process_search
                .as_ref()
                .map(|search| search.query.clone()),
            process_search_matches: process_search_match_ids.as_ref().map_or(0, HashSet::len),
            process_search_available: active_activities
                .iter()
                .any(|display| matches!(display.activity.variant, ActivityVariant::Process(_))),
            selected_disclosure: selected_id.and_then(|id| {
                model
                    .collapsible_descendant_count(id, ui_state)
                    .map(|_| !ui_state.expanded_activities.contains(&id))
            }),
            selected_has_logs: selected_id.is_some_and(|id| {
                model
                    .get_build_logs(id)
                    .is_some_and(|logs| !logs.is_empty())
            }),
            selected_preview_focused: selected_id
                .is_some_and(|id| ui_state.inline_logs_activity == Some(id)),
            process_previews_fit,
        })) {
            SummaryView
        }
    }
    .into_any();

    // Build the activity list element
    let activity_list = element! {
        View(flex_direction: FlexDirection::Column, width: 100pct) {
            #(activity_elements)
        }
    }
    .into_any();

    let statusline_position = ui_state.preferences.statusline.position;
    let sticky_statusline = show_summary && statusline_position != StatuslinePosition::Inline;
    let flowing_statusline =
        show_summary && live_layout && statusline_position == StatuslinePosition::Inline;
    let mut summary_bar = show_summary.then(|| {
        element! {
            View(
                height: 1,
                flex_shrink: 0.0,
                padding_left: 1,
                padding_right: 1
            ) {
                #(summary_view)
            }
        }
        .into_any()
    });
    let mut summary_gap =
        show_summary.then(|| element!(View(height: 1, flex_shrink: 0.0)).into_any());
    let mut children = vec![];

    if statusline_position == StatuslinePosition::Top
        && let Some(summary_bar) = summary_bar.take()
    {
        children.push(summary_bar);
        children.push(summary_gap.take().unwrap());
    }

    // Activity list: wrap in ScrollView for Normal render with scroll_handle,
    // use plain layout for Final render
    if let Some(handle) = scroll_handle {
        let scroll_height = available_activity_height(ui_state) as u32;
        children.push(
            element! {
                View(height: scroll_height) {
                    ScrollView(auto_scroll: true, keyboard_scroll: false, handle: handle) {
                        #(activity_list)
                    }
                }
            }
            .into_any(),
        );
    } else {
        children.push(
            element! {
                View(flex_grow: if flowing_statusline { 0.0_f32 } else { 1.0_f32 }, width: 100pct) {
                    #(activity_list)
                }
            }
            .into_any(),
        );
    }

    if let Some(summary_gap) = summary_gap {
        children.push(summary_gap);
    }
    if let Some(summary_bar) = summary_bar {
        children.push(summary_bar);
    }

    element! {
        ContextProvider(value: Context::owned(terminal_size)) {
            View(
                flex_direction: FlexDirection::Column,
                height: if sticky_statusline { Size::Length(terminal_size.height as u32) } else { Size::Auto },
                max_height: terminal_size.height as u32,
                width: 100pct,
                overflow: Overflow::Hidden,
                justify_content: if flowing_statusline { JustifyContent::FlexStart } else { JustifyContent::FlexEnd },
            ) {
                #(children)
            }
        }
    }
}

/// Context for activity rendering
#[derive(Clone)]
struct ActivityDisclosure {
    collapsed: bool,
    descendant_count: usize,
}

#[derive(Clone)]
struct ActivityRenderContext {
    activity: Activity,
    depth: usize,
    tree_position: TreePosition,
    disclosure: Option<ActivityDisclosure>,
    is_selected: bool,
    is_dimmed: bool,
    show_inline_logs: bool,
    logs: Option<Arc<VecDeque<String>>>,
    /// Total log line count (not affected by buffer rotation)
    log_line_count: usize,
    log_preview_lines: usize,
    /// Completion state: None = active, Some(true) = success, Some(false) = failed
    completed: Option<bool>,
    /// Whether this activity's result was cached
    cached: bool,
    /// Whether this is the final render before exit
    render_context: RenderContext,
    /// Whether the application is shutting down (Ctrl-C pressed)
    shutting_down: bool,
    /// Number of direct children hidden by the `hide_stopped_processes` filter.
    hidden_children_count: usize,
}

fn activity_is_dimmed(
    activity: &Activity,
    process_search_match_ids: Option<&HashSet<u64>>,
) -> bool {
    process_search_match_ids.is_some_and(|matches| {
        matches!(activity.variant, ActivityVariant::Process(_)) && !matches.contains(&activity.id)
    })
}

fn disclosed_name(activity: &Activity, disclosure: Option<&ActivityDisclosure>) -> String {
    match disclosure {
        Some(disclosure) if disclosure.collapsed => format!("▸ {}", activity.name),
        Some(_) => format!("▾ {}", activity.name),
        None => activity.name.clone(),
    }
}

fn disclosed_suffix(
    suffix: Option<String>,
    disclosure: Option<&ActivityDisclosure>,
) -> Option<String> {
    let Some(disclosure) = disclosure.filter(|disclosure| disclosure.collapsed) else {
        return suffix;
    };
    let summary = if disclosure.descendant_count == 1 {
        "1 step".to_string()
    } else {
        format!("{} steps", disclosure.descendant_count)
    };
    Some(match suffix {
        Some(suffix) => format!("{summary} • {suffix}"),
        None => summary,
    })
}

/// Helper to build activity prefix with hierarchy and status indicator.
/// - Top-level (depth == 0): [StatusIndicator]
/// - Nested (depth > 0): [HierarchyPrefix][StatusIndicator]
fn build_activity_prefix(
    depth: usize,
    tree_position: &TreePosition,
    completed: Option<bool>,
    show_spinner: bool,
) -> Vec<AnyElement<'static>> {
    let mut prefix = HierarchyPrefixComponent::new(
        depth,
        &tree_position.ancestor_continuations,
        tree_position.is_last_sibling,
    )
    .render();

    prefix.push(
        element!(View(margin_right: 1) {
            StatusIndicator(completed: completed, show_spinner: show_spinner)
        })
        .into_any(),
    );

    prefix
}

/// Like `build_activity_prefix`, but renders a process status dot whose shape
/// encodes the lifecycle state (see `process_status_dot`) instead of a spinner.
fn build_process_prefix(
    depth: usize,
    tree_position: &TreePosition,
    glyph: &'static str,
    color: Color,
    pulse: bool,
) -> Vec<AnyElement<'static>> {
    let mut prefix = HierarchyPrefixComponent::new(
        depth,
        &tree_position.ancestor_continuations,
        tree_position.is_last_sibling,
    )
    .render();

    prefix.push(
        element!(View(margin_right: 1) {
            StatusDot(glyph: glyph.to_string(), color: Some(color), pulse: pulse)
        })
        .into_any(),
    );

    prefix
}

fn process_preview_prefix(depth: usize, tree_position: &TreePosition) -> String {
    let mut prefix = "  ".to_string();
    for continues in &tree_position.ancestor_continuations {
        prefix.push_str(if *continues { "│ " } else { "  " });
    }
    if depth > 0 {
        prefix.push_str(if tree_position.is_last_sibling {
            "  "
        } else {
            "│ "
        });
    }
    prefix
}

/// Render a single activity (owned version)
#[component]
fn ActivityItem(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let terminal_width = hooks.use_context::<TerminalSize>().width;
    let ctx = hooks.use_context::<ActivityRenderContext>();

    // Measure rendered height and report it to the shared heights map.
    // Copy the iocraft Ref (which is Copy) out of the cell::Ref so we can call write().
    let heights = hooks.try_use_context::<ActivityHeights>().map(|r| *r);
    let rect = hooks.use_component_rect();
    if let (Some(mut heights), Some(rect)) = (heights, rect) {
        let height = rect.bottom.saturating_sub(rect.top);
        heights.write().insert(ctx.activity.id, height);
    }

    let ActivityRenderContext {
        activity,
        depth,
        tree_position,
        disclosure,
        is_selected,
        is_dimmed,
        show_inline_logs,
        logs,
        log_line_count,
        log_preview_lines,
        completed,
        cached,
        render_context,
        shutting_down,
        hidden_children_count,
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

            if *show_inline_logs {
                let prefix = build_activity_prefix(*depth, tree_position, *completed, true);

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
                    .with_terminal_width(terminal_width)
                    .with_max_lines(*log_preview_lines)
                    .with_empty_message("  → no build logs yet (press Ctrl-E to expand)")
                    .render_with_main_line(main_line);
            }

            // Non-selected build activities use normal rendering
            let prefix = build_activity_prefix(*depth, tree_position, *completed, true);

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

            let prefix = build_activity_prefix(*depth, tree_position, *completed, true);

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
            if (task_data.show_output || task_failed || *show_inline_logs)
                && (logs.is_some() || *show_inline_logs)
            {
                let empty_message = if completed.is_some() {
                    "  → no output"
                } else {
                    "  → waiting for output..."
                };
                let mut component = ExpandedContentComponent::new(logs.as_deref())
                    .with_terminal_width(terminal_width)
                    .with_max_lines(*log_preview_lines)
                    .with_empty_message(empty_message);
                if task_failed {
                    component = component.with_max_lines(LOG_VIEWPORT_FAILED);
                } else if task_data.show_output && !show_inline_logs {
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
                    .with_hierarchy(
                        &tree_position.ancestor_continuations,
                        tree_position.is_last_sibling,
                    )
                    .with_completed(*completed)
                    .with_cached(*cached)
                    .render(terminal_width);
            } else if let Some(progress) = &activity.progress {
                // Use generic progress if available
                if progress.total.unwrap_or(0) > 0 {
                    return DownloadActivityComponent::new(activity, *depth, *is_selected)
                        .with_hierarchy(
                            &tree_position.ancestor_continuations,
                            tree_position.is_last_sibling,
                        )
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
                    let prefix = build_activity_prefix(*depth, tree_position, *completed, true);

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
                let prefix = build_activity_prefix(*depth, tree_position, *completed, true);

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
            let prefix = build_activity_prefix(*depth, tree_position, *completed, true);

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
            let prefix = build_activity_prefix(*depth, tree_position, *completed, true);

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
            let prefix = build_activity_prefix(*depth, tree_position, *completed, true);

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
            } else if eval_data.files_read > 0 {
                Some(format!("{} files", eval_data.files_read))
            } else {
                activity.detail.clone()
            };
            let suffix = disclosed_suffix(suffix, disclosure.as_ref());

            let prefix = build_activity_prefix(*depth, tree_position, *completed, true);

            let main_line = ActivityTextComponent::name_only(
                disclosed_name(activity, disclosure.as_ref()),
                elapsed_str,
                activity.variant.clone(),
            )
            .with_suffix(suffix)
            .with_completed(completed.is_some())
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);

            let failed = *completed == Some(false);
            if *show_inline_logs || failed && logs.is_some() {
                let mut component = ExpandedContentComponent::new(logs.as_deref())
                    .with_terminal_width(terminal_width)
                    .with_max_lines(*log_preview_lines)
                    .with_empty_message("  → no files read yet (press Ctrl-E to expand)");
                if failed {
                    component = component.with_max_lines(LOG_VIEWPORT_FAILED);
                }
                return component.render_with_main_line(main_line);
            }

            return main_line;
        }
        ActivityVariant::UserOperation => {
            let prefix = build_activity_prefix(*depth, tree_position, *completed, true);

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
            let prefix = build_activity_prefix(*depth, tree_position, *completed, true);

            // Show line count as suffix when active or failed with logs
            let base_suffix = if *completed == Some(true) {
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

            let suffix = if *hidden_children_count > 0 {
                let hidden_note = format!("({} hidden)", hidden_children_count);
                Some(match base_suffix {
                    Some(s) => format!("{} {}", s, hidden_note),
                    None => hidden_note,
                })
            } else {
                base_suffix
            };
            let suffix = disclosed_suffix(suffix, disclosure.as_ref());

            let main_line = ActivityTextComponent::name_only(
                disclosed_name(activity, disclosure.as_ref()),
                elapsed_str,
                activity.variant.clone(),
            )
            .with_suffix(suffix)
            .with_completed(completed.is_some())
            .with_selection(*is_selected)
            .render(terminal_width, *depth, prefix);

            let failed = *completed == Some(false);
            if *show_inline_logs || failed && logs.is_some() {
                let mut component = ExpandedContentComponent::new(logs.as_deref())
                    .with_terminal_width(terminal_width)
                    .with_max_lines(*log_preview_lines)
                    .with_empty_message("  → no output yet");
                if failed {
                    component = component.with_max_lines(LOG_VIEWPORT_FAILED);
                }
                return component.render_with_main_line(main_line);
            }

            return main_line;
        }
        ActivityVariant::Process(process_data) => {
            let is_active = process_data.status.is_active();

            // Build status text with optional ports and proxy URLs.
            let status_str: std::borrow::Cow<str> = match &process_data.status {
                _ if *shutting_down && is_active => "stopping".into(),
                ProcessStatus::NotStarted => "auto start off".into(),
                ProcessStatus::Waiting => "waiting".into(),
                ProcessStatus::Starting => "starting".into(),
                ProcessStatus::Running => {
                    if let Some(probe) = &process_data.ready_probe {
                        format!("running (not ready: {})", probe).into()
                    } else {
                        "running".into()
                    }
                }
                ProcessStatus::Ready => "ready".into(),
                ProcessStatus::Restarting => "restarting".into(),
                ProcessStatus::Stopping => "stopping".into(),
                ProcessStatus::Stopped if *completed == Some(false) => "failed".into(),
                ProcessStatus::Stopped => "stopped".into(),
                ProcessStatus::Exited if *completed == Some(false) => "failed".into(),
                ProcessStatus::Exited => "exited".into(),
                ProcessStatus::GaveUp => "gave up (crash loop)".into(),
            };

            // Format ports: show just the port numbers for brevity
            let ports_suffix = if process_data.ports.is_empty() {
                String::new()
            } else {
                let port_list: Vec<String> = process_data
                    .ports
                    .iter()
                    .map(|binding| format!(":{}", binding.port))
                    .collect();
                format!(" {}", port_list.join(", "))
            };

            let urls_suffix = if process_data.urls.is_empty() {
                String::new()
            } else {
                format!(" {}", process_data.urls.join(", "))
            };
            let status_text = Some(format!("{}{}{}", status_str, ports_suffix, urls_suffix));

            // Process prefix: a status dot whose shape encodes the lifecycle
            // state; transient states pulse instead of animating a spinner.
            let (dot_glyph, dot_color, dot_pulse) =
                process_status_dot(&process_data.status, *completed, *shutting_down);
            let dot_color = if *is_dimmed {
                COLOR_HIERARCHY
            } else {
                dot_color
            };
            let prefix =
                build_process_prefix(*depth, tree_position, dot_glyph, dot_color, dot_pulse);

            // Hide elapsed time for not-started/stopped processes
            let process_elapsed = if matches!(process_data.status, ProcessStatus::NotStarted)
                || (!is_active && completed.is_none())
            {
                String::new()
            } else {
                elapsed_str
            };

            let main_line = ActivityTextComponent::new(
                "".to_string(),
                activity.name.clone(),
                process_elapsed,
                activity.variant.clone(),
            )
            .with_suffix(status_text)
            .with_selection(*is_selected)
            .with_dimmed(*is_dimmed)
            .render(terminal_width, *depth, prefix);

            let process_failed = *completed == Some(false) || process_data.status.is_failed();
            if *show_inline_logs
                || (process_failed && *render_context == RenderContext::Final && logs.is_some())
            {
                let mut component = ExpandedContentComponent::new(logs.as_deref())
                    .with_terminal_width(terminal_width)
                    .with_border_indent(*depth * 2)
                    .with_line_prefix(process_preview_prefix(*depth, tree_position))
                    .with_max_lines(*log_preview_lines)
                    .with_empty_message("→ no output yet");
                if process_failed && *render_context == RenderContext::Final {
                    component = component.with_max_lines(LOG_VIEWPORT_FAILED);
                }

                let mut elements = vec![main_line];
                let log_elements = component.render();
                elements.extend(log_elements);

                return element! {
                    View(
                        flex_direction: FlexDirection::Column,
                        max_height: INLINE_LOG_MAX_VISUAL_ROWS,
                        overflow: Overflow::Hidden,
                    ) {
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
                                    View(flex_grow: 1.0_f32, min_width: 0, overflow: Overflow::Hidden) {
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
                                View(flex_grow: 1.0_f32, min_width: 0, overflow: Overflow::Hidden) {
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
                                View(flex_grow: 1.0_f32, min_width: 0, overflow: Overflow::Hidden) {
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
                        View(flex_grow: 1.0_f32, min_width: 0, overflow: Overflow::Hidden) {
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
                        View(flex_grow: 1.0_f32, min_width: 0, overflow: Overflow::Hidden) {
                            Text(content: activity.name.clone(), color: selected_text_color)
                        }
                    }
                }
                .into_any();
            }
        }
        ActivityVariant::Unknown => {
            let prefix = build_activity_prefix(*depth, tree_position, *completed, true);

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
    preferences: Arc<crate::config::TuiPreferences>,
    keymap: Arc<crate::config::Keymap>,
    run_context: Arc<crate::config::TuiRunContext>,
    pending_key: Option<String>,
    summary: ActivitySummary,
    selected: Option<Activity>,
    showing_logs: bool,
    can_go_up: bool,
    can_go_down: bool,
    interrupt_prompt_active: bool,
    interrupt_prompt_attached: bool,
    hide_stopped_processes: bool,
    process_search: Option<String>,
    process_search_matches: usize,
    process_search_available: bool,
    selected_disclosure: Option<bool>,
    selected_has_logs: bool,
    selected_preview_focused: bool,
    process_previews_fit: bool,
}

/// Summary view component that adapts to terminal width
#[component]
fn SummaryView(hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let terminal_width = hooks.use_context::<TerminalSize>().width;
    let ctx = hooks.use_context::<SummaryViewContext>();
    build_summary_view_impl(&ctx, terminal_width)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FooterMode {
    Full,
    Compact,
    Symbols,
    Keys,
}

impl FooterMode {
    fn uses_symbols(self) -> bool {
        matches!(self, Self::Symbols | Self::Keys)
    }

    fn uses_short_text(self) -> bool {
        self != Self::Full
    }
}

struct FooterSpan {
    content: String,
    color: Option<Color>,
    weight: Weight,
}

impl FooterSpan {
    fn plain(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            color: None,
            weight: Weight::Normal,
        }
    }

    fn colored(content: impl Into<String>, color: Color) -> Self {
        Self {
            content: content.into(),
            color: Some(color),
            weight: Weight::Normal,
        }
    }

    fn completed(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            color: Some(COLOR_COMPLETED),
            weight: Weight::Bold,
        }
    }

    fn width(&self) -> usize {
        UnicodeWidthStr::width(self.content.as_str())
    }

    fn into_element(self) -> AnyElement<'static> {
        let width = self.width() as u32;
        element!(View(width: width, flex_shrink: 0.0) {
            Text(
                content: self.content,
                color: self.color,
                weight: self.weight,
                wrap: TextWrap::NoWrap,
            )
        })
        .into_any()
    }
}

struct FooterContent {
    spans: Vec<FooterSpan>,
}

impl FooterContent {
    fn width(&self) -> usize {
        self.spans.iter().map(FooterSpan::width).sum()
    }

    fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    fn into_elements(self) -> Vec<AnyElement<'static>> {
        self.spans
            .into_iter()
            .map(FooterSpan::into_element)
            .collect()
    }
}

struct FooterHelpContext {
    has_selection: bool,
    is_process: bool,
    is_stoppable: bool,
    is_restartable: bool,
    show_hide_toggle: bool,
    showing_logs: bool,
    can_go_up: bool,
    can_go_down: bool,
    hide_stopped_processes: bool,
    process_search_available: bool,
    selected_disclosure: Option<bool>,
    selected_has_logs: bool,
    selected_preview_focused: bool,
    process_previews_fit: bool,
}

fn build_summary_content(summary: &ActivitySummary, mode: FooterMode) -> FooterContent {
    let compact = mode.uses_symbols();
    let mut metrics = Vec::new();
    let builds = summary.active_builds + summary.completed_builds + summary.failed_builds;
    if builds > 0 {
        let total = summary
            .expected_builds
            .map(|value| value as usize)
            .unwrap_or(builds)
            .max(builds);
        metrics.push((
            summary.completed_builds.to_string(),
            if compact {
                format!("/{total} builds")
            } else {
                format!(" of {total} builds")
            },
        ));
    }
    let downloads = summary.active_downloads + summary.completed_downloads;
    if downloads > 0 {
        let total = summary
            .expected_downloads
            .map(|value| value as usize)
            .unwrap_or(downloads)
            .max(downloads);
        metrics.push((
            summary.completed_downloads.to_string(),
            if compact {
                format!("/{total} downloads")
            } else {
                format!(" of {total} downloads")
            },
        ));
    }
    let queries = summary.active_queries + summary.completed_queries;
    if queries > 0 {
        metrics.push((
            summary.completed_queries.to_string(),
            if compact {
                format!("/{queries} queries")
            } else {
                format!(" of {queries} queries")
            },
        ));
    }
    let tasks = summary.running_tasks + summary.completed_tasks + summary.failed_tasks;
    if tasks > 0 {
        metrics.push((
            summary.completed_tasks.to_string(),
            if compact {
                format!("/{tasks} tasks")
            } else {
                format!(" of {tasks} tasks")
            },
        ));
    }
    if summary.total_processes > 0 {
        let label = if summary.total_processes == 1 {
            "process"
        } else {
            "processes"
        };
        metrics.push(if summary.running_processes == summary.total_processes {
            (summary.total_processes.to_string(), format!(" {label}"))
        } else {
            (
                summary.running_processes.to_string(),
                if compact {
                    format!("/{} {label}", summary.total_processes)
                } else {
                    format!(" of {} {label}", summary.total_processes)
                },
            )
        });
    }

    let mut spans = Vec::new();
    let separator = if compact { " │ " } else { "  │  " };
    for (index, (count, label)) in metrics.into_iter().enumerate() {
        if index > 0 {
            spans.push(FooterSpan::colored(separator, COLOR_HIERARCHY));
        }
        spans.push(FooterSpan::completed(count));
        spans.push(FooterSpan::plain(label));
    }
    FooterContent { spans }
}

fn build_help_content(ctx: &FooterHelpContext, mode: FooterMode) -> FooterContent {
    let up_arrow_color = if ctx.can_go_up {
        COLOR_INTERACTIVE
    } else {
        COLOR_HIERARCHY
    };
    let down_arrow_color = if ctx.can_go_down {
        COLOR_INTERACTIVE
    } else {
        COLOR_HIERARCHY
    };
    let mut spans = vec![
        FooterSpan::colored("↑", up_arrow_color),
        FooterSpan::colored("↓", down_arrow_color),
        FooterSpan::colored(" j", down_arrow_color),
        FooterSpan::colored("/", COLOR_HIERARCHY),
        FooterSpan::colored("k", up_arrow_color),
        FooterSpan::colored(
            if mode.uses_short_text() {
                " ^D"
            } else {
                " Ctrl-D"
            },
            down_arrow_color,
        ),
        FooterSpan::colored("/", COLOR_HIERARCHY),
        FooterSpan::colored(
            if mode.uses_short_text() {
                "^U"
            } else {
                "Ctrl-U"
            },
            up_arrow_color,
        ),
    ];

    if ctx.has_selection && mode == FooterMode::Keys {
        spans.push(FooterSpan::colored(" Enter", COLOR_INTERACTIVE));
        if ctx.selected_has_logs || ctx.is_process {
            spans.push(FooterSpan::colored(" ^E", COLOR_INTERACTIVE));
        }
        if ctx.is_stoppable {
            spans.push(FooterSpan::colored(" ^X", COLOR_INTERACTIVE));
        }
        if ctx.is_restartable {
            spans.push(FooterSpan::colored(" ^R", COLOR_INTERACTIVE));
        }
        if ctx.show_hide_toggle {
            spans.push(FooterSpan::colored(" ^H", COLOR_INTERACTIVE));
        }
        if ctx.process_search_available {
            spans.push(FooterSpan::colored(" /", COLOR_INTERACTIVE));
        }
        spans.push(FooterSpan::colored(" Esc", COLOR_INTERACTIVE));
        return FooterContent { spans };
    }

    if ctx.has_selection {
        if mode == FooterMode::Symbols {
            spans.push(FooterSpan::plain(" • "));
        } else if mode == FooterMode::Compact {
            spans.push(FooterSpan::plain(" nav "));
        } else {
            spans.push(FooterSpan::plain(" navigate • "));
        }
        let directional_open =
            ctx.selected_disclosure == Some(true) || ctx.is_process && !ctx.showing_logs;
        let directional_close =
            ctx.selected_disclosure == Some(false) || ctx.is_process && ctx.showing_logs;
        spans.push(FooterSpan::colored(
            if directional_open && mode == FooterMode::Full {
                "Enter/l/→"
            } else if directional_close && mode == FooterMode::Full {
                "Enter/h/←"
            } else {
                "Enter"
            },
            COLOR_INTERACTIVE,
        ));
        spans.push(FooterSpan::plain(match ctx.selected_disclosure {
            Some(true) if mode == FooterMode::Symbols => " ▾ • ",
            Some(false) if mode == FooterMode::Symbols => " ▴ • ",
            Some(true) => " expand ",
            Some(false) => " collapse ",
            None if ctx.is_process
                && !ctx.showing_logs
                && ctx.process_previews_fit
                && mode == FooterMode::Symbols =>
            {
                " ◎ • "
            }
            None if ctx.is_process && ctx.showing_logs && mode == FooterMode::Symbols => " ▴ • ",
            None if mode == FooterMode::Symbols => " ▾ • ",
            None if ctx.is_process && !ctx.showing_logs && ctx.process_previews_fit => " focus ",
            None if ctx.is_process
                && ctx.showing_logs
                && ctx.process_previews_fit
                && !ctx.selected_preview_focused =>
            {
                " hide previews "
            }
            None if ctx.is_process && ctx.showing_logs => " hide preview ",
            None if mode == FooterMode::Compact => " preview ",
            None => " preview logs • ",
        }));
        if ctx.selected_has_logs || ctx.is_process {
            spans.push(FooterSpan::colored(
                if mode.uses_short_text() {
                    "^E"
                } else {
                    "Ctrl-E"
                },
                COLOR_INTERACTIVE,
            ));
            spans.push(FooterSpan::plain(if mode == FooterMode::Symbols {
                " ▼ • "
            } else if mode == FooterMode::Compact {
                " logs "
            } else {
                " full logs • "
            }));
        }
        if ctx.is_process {
            if ctx.is_stoppable {
                spans.push(FooterSpan::colored(
                    if mode.uses_short_text() {
                        "^X"
                    } else {
                        "Ctrl-X"
                    },
                    COLOR_INTERACTIVE,
                ));
                spans.push(FooterSpan::plain(if mode == FooterMode::Symbols {
                    " "
                } else if mode == FooterMode::Compact {
                    " stop "
                } else {
                    " stop process • "
                }));
            }
            if ctx.is_restartable {
                spans.push(FooterSpan::colored(
                    if mode.uses_short_text() {
                        "^R"
                    } else {
                        "Ctrl-R"
                    },
                    COLOR_INTERACTIVE,
                ));
                spans.push(FooterSpan::plain(if mode == FooterMode::Symbols {
                    " "
                } else if mode == FooterMode::Compact {
                    " restart "
                } else {
                    " (re)start process • "
                }));
            }
        }
        if ctx.show_hide_toggle {
            spans.push(FooterSpan::colored(
                if mode.uses_short_text() {
                    "^H"
                } else {
                    "Ctrl-H"
                },
                COLOR_INTERACTIVE,
            ));
            spans.push(FooterSpan::plain(if mode == FooterMode::Symbols {
                " "
            } else if mode == FooterMode::Compact {
                if ctx.hide_stopped_processes {
                    " show "
                } else {
                    " hide "
                }
            } else if ctx.hide_stopped_processes {
                " show stopped • "
            } else {
                " hide stopped • "
            }));
        }
        if ctx.process_search_available {
            spans.push(FooterSpan::colored("/", COLOR_INTERACTIVE));
            spans.push(FooterSpan::plain(if mode == FooterMode::Symbols {
                " "
            } else if mode == FooterMode::Compact {
                " search "
            } else {
                " search processes • "
            }));
        }
        spans.push(FooterSpan::colored("Esc", COLOR_INTERACTIVE));
        spans.push(FooterSpan::plain(if ctx.showing_logs {
            if mode == FooterMode::Symbols {
                " ✕"
            } else if mode == FooterMode::Compact {
                " hide"
            } else {
                " hide logs"
            }
        } else {
            " clear"
        }));
    } else {
        let trail = ctx.show_hide_toggle || ctx.process_search_available;
        if mode == FooterMode::Compact {
            spans.push(FooterSpan::plain(if trail { " nav • " } else { " nav" }));
        } else if mode == FooterMode::Full {
            spans.push(FooterSpan::plain(if trail {
                " navigate • "
            } else {
                " navigate"
            }));
        } else if trail {
            spans.push(FooterSpan::plain(" • "));
        }
        if ctx.show_hide_toggle {
            spans.push(FooterSpan::colored(
                if mode.uses_short_text() {
                    "^H"
                } else {
                    "Ctrl-H"
                },
                COLOR_INTERACTIVE,
            ));
            if mode != FooterMode::Symbols {
                spans.push(FooterSpan::plain(if ctx.hide_stopped_processes {
                    " show stopped"
                } else {
                    " hide stopped"
                }));
            }
        }
        if ctx.process_search_available {
            if ctx.show_hide_toggle {
                spans.push(FooterSpan::plain(" • "));
            }
            spans.push(FooterSpan::colored("/", COLOR_INTERACTIVE));
            if mode != FooterMode::Symbols {
                spans.push(FooterSpan::plain(" search"));
            }
        }
    }
    FooterContent { spans }
}

fn footer_modes(has_selection: bool, terminal_width: u16) -> &'static [FooterMode] {
    if has_selection {
        if terminal_width < 60 {
            &[FooterMode::Keys]
        } else if terminal_width < 120 {
            &[FooterMode::Symbols, FooterMode::Keys]
        } else if terminal_width < 200 {
            &[FooterMode::Compact, FooterMode::Symbols, FooterMode::Keys]
        } else {
            &[
                FooterMode::Full,
                FooterMode::Compact,
                FooterMode::Symbols,
                FooterMode::Keys,
            ]
        }
    } else if terminal_width < 100 {
        &[FooterMode::Symbols]
    } else if terminal_width < 160 {
        &[FooterMode::Compact, FooterMode::Symbols]
    } else {
        &[FooterMode::Full, FooterMode::Compact, FooterMode::Symbols]
    }
}

fn build_default_footer(
    summary: &ActivitySummary,
    help_ctx: &FooterHelpContext,
    terminal_width: u16,
) -> AnyElement<'static> {
    let content_width = usize::from(terminal_width.saturating_sub(2));
    let modes = footer_modes(help_ctx.has_selection, terminal_width);
    let summary_allowed = !help_ctx.has_selection || terminal_width >= 72;
    let fitted_mode = summary_allowed
        .then(|| {
            modes.iter().copied().find(|mode| {
                let summary = build_summary_content(summary, *mode);
                let help = build_help_content(help_ctx, *mode);
                let gap = if summary.is_empty() || mode.uses_symbols() {
                    1
                } else {
                    2
                };
                summary.width() + gap + help.width() <= content_width
            })
        })
        .flatten();
    let show_summary = fitted_mode.is_some();
    let mode = fitted_mode
        .or_else(|| {
            modes
                .iter()
                .copied()
                .find(|mode| build_help_content(help_ctx, *mode).width() <= content_width)
        })
        .unwrap_or_else(|| *modes.last().unwrap());
    let summary = if show_summary {
        build_summary_content(summary, mode)
    } else {
        FooterContent { spans: Vec::new() }
    };
    let help = build_help_content(help_ctx, mode);
    let gap = if summary.is_empty() || mode.uses_symbols() {
        1
    } else {
        2
    };
    let required_width = (summary.width() + gap + help.width()).min(content_width) as u32;
    let left = summary.into_elements();
    let right = help.into_elements();

    if help_ctx.has_selection && terminal_width < 72 {
        return element!(View(
            flex_direction: FlexDirection::Row,
            width: terminal_width.saturating_sub(2) as u32,
            overflow: Overflow::Hidden,
        ) {
            #(right)
        })
        .into_any();
    }

    element!(View(
        flex_direction: FlexDirection::Row,
        width: required_width,
        flex_grow: 1.0_f32,
        flex_shrink: 0.0,
        overflow: Overflow::Hidden,
    ) {
            View(flex_direction: FlexDirection::Row, flex_grow: 1.0_f32, min_width: 0, overflow: Overflow::Hidden) {
                #(left)
            }
            View(width: gap as u32, flex_shrink: 0.0)
            View(flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                #(right)
            }
    })
    .into_any()
}

/// Build the summary view with colored counts
fn build_summary_view_impl(ctx: &SummaryViewContext, terminal_width: u16) -> AnyElement<'static> {
    let SummaryViewContext {
        preferences,
        keymap,
        run_context,
        pending_key,
        summary,
        selected,
        showing_logs,
        can_go_up,
        can_go_down,
        interrupt_prompt_active,
        interrupt_prompt_attached,
        hide_stopped_processes,
        process_search,
        process_search_matches,
        process_search_available,
        selected_disclosure,
        selected_has_logs,
        selected_preview_focused,
        process_previews_fit,
    } = ctx;
    let selected = selected.as_ref();
    let showing_logs = *showing_logs;
    let can_go_up = *can_go_up;
    let can_go_down = *can_go_down;
    let interrupt_prompt_active = *interrupt_prompt_active;
    let interrupt_prompt_attached = *interrupt_prompt_attached;
    let hide_stopped_processes = *hide_stopped_processes;
    let process_search_matches = *process_search_matches;
    let process_search_available = *process_search_available;
    let selected_disclosure = *selected_disclosure;
    let selected_has_logs = *selected_has_logs;
    let selected_preview_focused = *selected_preview_focused;
    let process_previews_fit = *process_previews_fit;
    let has_selection = selected.is_some();
    let is_process =
        matches!(selected, Some(a) if matches!(a.variant, ActivityVariant::Process(_)));
    let is_stoppable = matches!(
        selected,
        Some(a) if matches!(&a.variant, ActivityVariant::Process(p) if p.status.is_stoppable())
    );
    let is_restartable = matches!(
        selected,
        Some(a) if matches!(&a.variant, ActivityVariant::Process(p) if p.status.is_restartable())
    );
    let show_hide_toggle = summary.stopped_processes > 0 || hide_stopped_processes;

    let configured_statusline = !preferences.uses_default_statusline();
    if configured_statusline {
        let (mode, prompt, key_hints) = if interrupt_prompt_active {
            (
                StatuslineMode::Prompt,
                Some(if interrupt_prompt_attached {
                    "Detach or stop the process manager?".to_string()
                } else {
                    "Quit devenv? Nothing has been stopped yet.".to_string()
                }),
                Some(interrupt_prompt_key_hints(
                    keymap,
                    interrupt_prompt_attached,
                    terminal_width,
                )),
            )
        } else if process_search.is_some() {
            (StatuslineMode::Search, None, None)
        } else {
            (StatuslineMode::Main, None, None)
        };
        let key_context = match mode {
            StatuslineMode::Main => crate::config::KeyContext::Main,
            StatuslineMode::Search => crate::config::KeyContext::ProcessSearch,
            StatuslineMode::Prompt => crate::config::KeyContext::Prompt,
            StatuslineMode::Logs => crate::config::KeyContext::Logs,
        };
        let actions = match mode {
            StatuslineMode::Main => {
                let mut actions = vec![
                    crate::config::Action::MoveDown,
                    crate::config::Action::MoveUp,
                    crate::config::Action::HalfPageDown,
                    crate::config::Action::HalfPageUp,
                ];
                if has_selection {
                    actions.push(crate::config::Action::Activate);
                    if selected_disclosure == Some(true) || is_process && !showing_logs {
                        actions.push(crate::config::Action::Expand);
                    }
                    if selected_disclosure == Some(false) || is_process && showing_logs {
                        actions.push(crate::config::Action::Collapse);
                    }
                    if selected_has_logs || is_process {
                        actions.push(crate::config::Action::OpenLogs);
                    }
                    if is_stoppable {
                        actions.push(crate::config::Action::StopProcess);
                    }
                    if is_restartable {
                        actions.push(crate::config::Action::RestartProcess);
                    }
                    actions.push(crate::config::Action::Cancel);
                }
                if show_hide_toggle {
                    actions.push(crate::config::Action::ToggleStopped);
                }
                if process_search_available {
                    actions.push(crate::config::Action::Search);
                }
                actions
            }
            StatuslineMode::Search => vec![
                crate::config::Action::NextMatch,
                crate::config::Action::PreviousMatch,
                crate::config::Action::Accept,
                crate::config::Action::Cancel,
            ],
            StatuslineMode::Prompt => Vec::new(),
            StatuslineMode::Logs => Vec::new(),
        };
        let key_hints =
            key_hints.or_else(|| action_key_hints(keymap, key_context, actions, terminal_width));
        let data = StatuslineData {
            summary: summary.clone(),
            selected: selected.cloned(),
            context: (**run_context).clone(),
            hidden_processes: if hide_stopped_processes {
                summary.stopped_processes
            } else {
                0
            },
            search_query: process_search.clone(),
            search_total: Some(process_search_matches),
            search_result: Some(match process_search_matches {
                0 => "no matches".to_string(),
                1 => "1 match".to_string(),
                count => format!("{count} matches"),
            }),
            prompt,
            pending_key: pending_key.clone(),
            key_hints,
            ..StatuslineData::default()
        };
        return build_configured_statusline(
            render_statusline(mode, terminal_width.saturating_sub(2), preferences, &data),
            &preferences.theme,
            terminal_width,
        );
    }

    if interrupt_prompt_active {
        if interrupt_prompt_attached {
            // Attached to a running manager: Ctrl-C detaches (leaves processes
            // running), `s` stops the whole manager, Esc keeps watching.
            let prompt_text = if terminal_width < 60 {
                "Detach?"
            } else if terminal_width < 100 {
                "Detach or stop the manager?"
            } else {
                "Detach or stop the process manager?"
            };
            let compact = terminal_width < 84;
            return element!(View(
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                width: 100pct
            ) {
                View(flex_grow: 1.0_f32, flex_shrink: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                    Text(content: prompt_text, color: Color::Yellow, weight: Weight::Bold)
                }
                View(flex_direction: FlexDirection::Row, flex_shrink: 1.0, min_width: 0, overflow: Overflow::Hidden, margin_left: 2) {
                    Text(content: "Ctrl-C", color: COLOR_INTERACTIVE)
                    Text(content: if compact { ":detach " } else { " detach • " })
                    Text(content: "s", color: COLOR_INTERACTIVE)
                    Text(content: if compact { ":stop " } else { " stop manager • " })
                    Text(content: "Esc", color: COLOR_INTERACTIVE)
                    Text(content: if compact { ":watch" } else { " keep watching" })
                }
            })
            .into_any();
        }

        // Pick prompt verbosity so the prompt plus the (non-shrinking) key hints
        // fit within the terminal width. The full hints occupy ~39 columns, so
        // the long prompt only fits comfortably on wide terminals.
        let prompt_text = if terminal_width < 60 {
            "Quit?"
        } else if terminal_width < 100 {
            "Quit devenv? Nothing stopped."
        } else {
            "Quit devenv? Nothing has been stopped yet."
        };
        // Compact, single-spaced hints on narrow terminals so the row never
        // overflows; verbose hints once there is room.
        let compact = terminal_width < 72;

        return element!(View(
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            width: 100pct
        ) {
            View(flex_grow: 1.0_f32, flex_shrink: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                Text(content: prompt_text, color: Color::Yellow, weight: Weight::Bold)
            }
            View(flex_direction: FlexDirection::Row, flex_shrink: 1.0, min_width: 0, overflow: Overflow::Hidden, margin_left: 2) {
                Text(content: "c", color: COLOR_INTERACTIVE)
                Text(content: if compact { ":run " } else { " keep running • " })
                Text(content: "q", color: COLOR_INTERACTIVE)
                Text(content: if compact { ":quit " } else { " quit • " })
                Text(content: "Ctrl-C", color: COLOR_INTERACTIVE)
                Text(content: ":quit")
            }
        })
        .into_any();
    }

    if let Some(query) = process_search {
        let prompt = if query.is_empty() {
            "Search processes: /".to_string()
        } else {
            format!("Search processes: /{query}")
        };
        let result = match process_search_matches {
            0 => "no matches".to_string(),
            1 => "1 match".to_string(),
            count => format!("{count} matches"),
        };
        let hints = if terminal_width < 72 {
            format!("{result}  Enter:select Esc:cancel")
        } else {
            format!("{result} • ↑↓ next • Enter select • Esc cancel")
        };
        let prompt_color = process_search_prompt_color(process_search_matches);
        return element!(View(
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            width: 100pct,
            overflow: Overflow::Hidden,
        ) {
            View(flex_grow: 1.0_f32, flex_shrink: 0.0, min_width: 0, overflow: Overflow::Hidden) {
                Text(content: prompt, color: prompt_color, weight: Weight::Bold)
            }
            View(flex_shrink: 1.0, min_width: 0, overflow: Overflow::Hidden, margin_left: 2) {
                Text(content: hints)
            }
        })
        .into_any();
    }

    let footer_help = FooterHelpContext {
        has_selection,
        is_process,
        is_stoppable,
        is_restartable,
        show_hide_toggle,
        showing_logs,
        can_go_up,
        can_go_down,
        hide_stopped_processes,
        process_search_available,
        selected_disclosure,
        selected_has_logs,
        selected_preview_focused,
        process_previews_fit,
    };
    build_default_footer(summary, &footer_help, terminal_width)
}
pub(crate) fn build_configured_statusline(
    statusline: crate::statusline::RenderedStatusline,
    theme: &crate::config::ThemeConfig,
    terminal_width: u16,
) -> AnyElement<'static> {
    let separator = statusline.separator;
    let group_width = |segments: &[RenderedSegment]| {
        segments
            .iter()
            .map(|segment| UnicodeWidthStr::width(segment.content.as_str()))
            .sum::<usize>()
            + UnicodeWidthStr::width(separator.as_str()) * segments.len().saturating_sub(1)
    };
    let inner_width = usize::from(terminal_width.saturating_sub(2));
    let left_width = group_width(&statusline.left);
    let center_width = group_width(&statusline.center);
    let right_width = group_width(&statusline.right);
    let left_center_gap = usize::from(left_width > 0 && center_width > 0);
    let center_right_gap = usize::from(center_width > 0 && right_width > 0);
    let left_right_gap = usize::from(center_width == 0 && left_width > 0 && right_width > 0);
    let right_start = inner_width.saturating_sub(right_width);
    let center_target = inner_width.saturating_sub(center_width) / 2;
    let center_min = left_width.saturating_add(left_center_gap);
    let center_max = right_start.saturating_sub(center_width.saturating_add(center_right_gap));
    let center_start = if center_min <= center_max {
        center_target.clamp(center_min, center_max)
    } else {
        center_min
    };
    let first_spacer = if center_width > 0 {
        center_start.saturating_sub(left_width)
    } else {
        right_start.saturating_sub(left_width).max(left_right_gap)
    };
    let second_spacer = if center_width > 0 {
        right_start.saturating_sub(center_start.saturating_add(center_width))
    } else {
        0
    };
    let separator_style = separator_style(theme);
    let build_group = |segments: Vec<RenderedSegment>| {
        let mut children = Vec::new();
        for (index, segment) in segments.into_iter().enumerate() {
            if index > 0 {
                children.push(
                    element!(View(background_color: separator_style.background, flex_shrink: 0.0) {
                        Text(
                            content: separator.clone(),
                            color: separator_style.foreground,
                            weight: if separator_style.bold { Weight::Bold } else if separator_style.dim { Weight::Light } else { Weight::Normal },
                            decoration: if separator_style.underline { TextDecoration::Underline } else { TextDecoration::None },
                            italic: separator_style.italic,
                            invert: separator_style.reverse,
                        )
                    })
                    .into_any(),
                );
            }
            let style = segment.style;
            children.push(
                element!(View(background_color: style.background, flex_shrink: 0.0) {
                    Text(
                        content: segment.content,
                        color: style.foreground,
                        weight: if style.bold { Weight::Bold } else if style.dim { Weight::Light } else { Weight::Normal },
                        decoration: if style.underline { TextDecoration::Underline } else { TextDecoration::None },
                        italic: style.italic,
                        invert: style.reverse,
                    )
                })
                .into_any(),
            );
        }
        children
    };
    let left = build_group(statusline.left);
    let center = build_group(statusline.center);
    let right = build_group(statusline.right);
    element!(View(
        flex_direction: FlexDirection::Row,
        width: terminal_width.saturating_sub(2) as u32,
        overflow: Overflow::Hidden,
    ) {
        View(flex_direction: FlexDirection::Row, flex_shrink: 0.0, overflow: Overflow::Hidden) { #(left) }
        View(width: first_spacer as u32, flex_shrink: 0.0)
        View(flex_direction: FlexDirection::Row, flex_shrink: 0.0, overflow: Overflow::Hidden) { #(center) }
        View(width: second_spacer as u32, flex_shrink: 0.0)
        View(flex_direction: FlexDirection::Row, flex_shrink: 0.0, overflow: Overflow::Hidden) { #(right) }
    })
    .into_any()
}

fn process_search_prompt_color(matches: usize) -> Color {
    if matches == 0 {
        COLOR_FAILED
    } else {
        COLOR_INTERACTIVE
    }
}

/// Format a duration in a human-readable way
pub fn format_duration(duration: Duration) -> String {
    if cfg!(feature = "deterministic-tui") {
        return "[TIME]".to_string();
    }
    duration.human_duration().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ActivityModel;
    use devenv_activity::{
        ActivityEvent, ActivityLevel, Operation, Process, Timestamp,
        test_helpers::{process_log, process_start, task_hierarchy_single, task_log, task_start},
    };

    #[test]
    fn tree_positions_connect_nested_siblings() {
        assert_eq!(
            tree_positions_for_depths(&[0, 1, 2, 2, 1, 2]),
            vec![
                TreePosition {
                    ancestor_continuations: vec![],
                    is_last_sibling: true,
                },
                TreePosition {
                    ancestor_continuations: vec![],
                    is_last_sibling: false,
                },
                TreePosition {
                    ancestor_continuations: vec![true],
                    is_last_sibling: false,
                },
                TreePosition {
                    ancestor_continuations: vec![true],
                    is_last_sibling: true,
                },
                TreePosition {
                    ancestor_continuations: vec![],
                    is_last_sibling: true,
                },
                TreePosition {
                    ancestor_continuations: vec![false],
                    is_last_sibling: true,
                },
            ]
        );
    }

    #[test]
    fn process_rows_show_proxy_urls_alongside_ports() {
        for (width, urls) in [
            (80, vec!["http://docs.devenv8.localhost"]),
            (
                160,
                vec!["http://app.localhost:8080", "http://admin.localhost:8080"],
            ),
        ] {
            let mut model = ActivityModel::default();
            let mut ui = UiState::new();
            ui.terminal_size.width = width;
            let mut event = process_start(1, "docs");
            if let ActivityEvent::Process(Process::Start {
                ports,
                urls: event_urls,
                ..
            }) = &mut event
            {
                *ports = vec![devenv_activity::PortBinding {
                    name: "http".to_owned(),
                    port: 4321,
                }];
                **event_urls = urls.iter().map(|url| (*url).to_owned()).collect();
            }
            // The same event is serialized for replay when attaching.
            let event = serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();
            model.apply_activity_event(event);
            model.apply_activity_event(ActivityEvent::Process(Process::Status {
                id: 1,
                status: ProcessStatus::Ready,
                timestamp: Timestamp::now(),
            }));

            let mut element: AnyElement<'static> =
                view(&model, &ui, RenderContext::Normal, None, false).into();
            let rendered = element.render(Some(width as usize)).to_string();
            assert!(rendered.contains("ready :4321"), "{rendered}");
            for url in urls {
                assert!(rendered.contains(url), "missing {url}: {rendered}");
            }
        }
    }

    #[test]
    fn process_preview_prefix_matches_tree_position() {
        assert_eq!(
            process_preview_prefix(
                1,
                &TreePosition {
                    ancestor_continuations: vec![],
                    is_last_sibling: false,
                },
            ),
            "  │ "
        );
        assert_eq!(
            process_preview_prefix(
                1,
                &TreePosition {
                    ancestor_continuations: vec![],
                    is_last_sibling: true,
                },
            ),
            "    "
        );
        assert_eq!(
            process_preview_prefix(
                2,
                &TreePosition {
                    ancestor_continuations: vec![true],
                    is_last_sibling: false,
                },
            ),
            "  │ │ "
        );
        assert_eq!(
            process_preview_prefix(
                2,
                &TreePosition {
                    ancestor_continuations: vec![false],
                    is_last_sibling: true,
                },
            ),
            "      "
        );
    }

    #[test]
    fn process_previews_do_not_fit_beside_multiline_task_output() {
        let mut model = ActivityModel::new();
        model.apply_activity_event(task_hierarchy_single(1, "build", None, true, false));
        model.apply_activity_event(task_start(1));
        model.apply_activity_event(task_log(1, "line one", false));
        model.apply_activity_event(task_log(1, "line two", false));
        model.apply_activity_event(task_log(1, "line three", false));
        model.apply_activity_event(process_start(2, "server"));
        model.apply_activity_event(process_log(2, "listening", false));

        let mut ui_state = UiState::new();
        ui_state.set_terminal_size(80, 7);
        let display = model.get_display_activities(&ui_state);

        // The task occupies four rows (its main row plus three output rows),
        // and the process with its preview occupies two more. The five-row
        // viewport therefore cannot show automatic previews without scrolling.
        assert!(!process_previews_fit(&model, &display, &ui_state));
    }

    #[test]
    fn configured_log_preview_lines_control_preview_fit() {
        let config = Arc::new(crate::app::TuiConfig {
            log_viewport_collapsed: 2,
            ..crate::app::TuiConfig::default()
        });
        let mut model = ActivityModel::with_config(config);
        model.apply_activity_event(process_start(1, "server"));
        for line in 0..5 {
            model.apply_activity_event(process_log(1, format!("line {line}"), false));
        }

        let mut ui_state = UiState::new();
        ui_state.set_terminal_size(80, 5);
        let display = model.get_display_activities(&ui_state);

        assert!(process_previews_fit(&model, &display, &ui_state));

        let mut element: AnyElement<'static> = view(
            &model,
            &ui_state,
            RenderContext::Normal,
            Some(ScrollState {
                handle: None,
                display_activities: display,
                process_previews_fit: true,
            }),
            false,
        )
        .into();
        let rendered = element.render(Some(80)).to_string();

        assert!(!rendered.contains("line 2"));
        assert!(rendered.contains("line 3"));
        assert!(rendered.contains("line 4"));
    }

    fn summary_ctx(
        hide_stopped_processes: bool,
        interrupt_prompt_active: bool,
        stopped_processes: usize,
    ) -> SummaryViewContext {
        SummaryViewContext {
            preferences: Arc::new(crate::config::TuiPreferences::default()),
            keymap: Arc::new(
                crate::config::KeybindingsConfig::default()
                    .resolve()
                    .unwrap(),
            ),
            run_context: Arc::new(crate::config::TuiRunContext::default()),
            pending_key: None,
            summary: ActivitySummary {
                stopped_processes,
                ..ActivitySummary::default()
            },
            selected: None,
            showing_logs: false,
            can_go_up: false,
            can_go_down: false,
            interrupt_prompt_active,
            interrupt_prompt_attached: false,
            hide_stopped_processes,
            process_search: None,
            process_search_matches: 0,
            process_search_available: false,
            selected_disclosure: None,
            selected_has_logs: false,
            selected_preview_focused: false,
            process_previews_fit: false,
        }
    }

    #[test]
    fn test_summary_interrupt_prompt_renders() {
        let mut element = build_summary_view_impl(&summary_ctx(false, true, 0), 100);
        let output = element.render(Some(100)).to_string();

        assert!(output.contains("Quit devenv?"));
        assert!(output.contains("stopped"));
        assert!(output.contains("keep running"));
        assert!(output.contains("quit"));
    }

    #[test]
    fn configured_interrupt_prompts_remain_visible_and_contextual() {
        let mut hidden = summary_ctx(false, true, 0);
        let mut preferences = crate::config::TuiPreferences::default();
        preferences.statusline.enabled = false;
        hidden.preferences = Arc::new(preferences);
        let hidden_output = build_summary_view_impl(&hidden, 160)
            .render(Some(160))
            .to_string();
        assert!(hidden_output.contains("Quit devenv?"));
        assert!(hidden_output.contains("q quit"));
        assert!(hidden_output.contains("Ctrl-C quit"));
        assert!(!hidden_output.contains("s stop manager"));

        let mut attached = summary_ctx(false, true, 0);
        attached.interrupt_prompt_attached = true;
        let mut preferences = crate::config::TuiPreferences::default();
        preferences.theme.preset = crate::config::ThemePreset::Terminal;
        attached.preferences = Arc::new(preferences);
        let attached_output = build_summary_view_impl(&attached, 160)
            .render(Some(160))
            .to_string();
        assert!(attached_output.contains("Detach or stop"));
        assert!(attached_output.contains("s stop manager"));
        assert!(attached_output.contains("Ctrl-C detach"));
        assert!(!attached_output.contains("q quit"));
    }

    #[test]
    fn configured_statusline_paints_separator_background() {
        let background = Color::Blue;
        let segment = |name: &str| RenderedSegment {
            name: name.to_string(),
            content: name.to_string(),
            style: crate::statusline::SegmentStyle {
                background: Some(background),
                ..crate::statusline::SegmentStyle::default()
            },
        };
        let statusline = crate::statusline::RenderedStatusline {
            left: vec![segment("one"), segment("two")],
            separator: " │ ".to_string(),
            ..crate::statusline::RenderedStatusline::default()
        };
        let mut theme = crate::config::ThemeConfig::default();
        theme.styles.insert(
            "statusline".to_string(),
            crate::config::StyleConfig {
                background: Some(crate::config::ColorSpec("blue".to_string())),
                ..crate::config::StyleConfig::default()
            },
        );

        let mut element = build_configured_statusline(statusline, &theme, 40);
        let canvas = element.render(Some(40));
        let separator_column = (0..canvas.width())
            .find(|column| canvas.cell(*column, 0).and_then(|cell| cell.text()) == Some("│"))
            .unwrap();
        assert_eq!(
            canvas.cell(separator_column, 0).unwrap().background_color,
            Some(background)
        );
    }

    #[test]
    fn configured_center_zone_uses_the_terminal_midpoint() {
        let segment = |name: &str| RenderedSegment {
            name: name.to_string(),
            content: name.to_string(),
            style: crate::statusline::SegmentStyle::default(),
        };
        let statusline = crate::statusline::RenderedStatusline {
            left: vec![segment("L")],
            center: vec![segment("CENTER")],
            right: vec![segment("RIGHT-RIGHT")],
            separator: " | ".to_string(),
        };
        let mut element =
            build_configured_statusline(statusline, &crate::config::ThemeConfig::default(), 42);
        let output = element.render(Some(42)).to_string();
        let line = output.lines().next().unwrap();
        let center = line.find("CENTER").unwrap();

        assert_eq!(
            center + UnicodeWidthStr::width("CENTER") / 2,
            20,
            "{line:?}"
        );
        assert_eq!(line.find("RIGHT-RIGHT").unwrap(), 29, "{line:?}");
    }

    #[test]
    fn disabled_statusline_only_reserves_rows_for_interactions() {
        let mut ui = UiState::new();
        let mut preferences = crate::config::TuiPreferences::default();
        preferences.statusline.enabled = false;
        ui.set_preferences(preferences).unwrap();

        assert_eq!(summary_bar_height(&ui), 0);
        assert_eq!(
            available_activity_height(&ui),
            usize::from(ui.terminal_size.height)
        );

        ui.start_process_search();
        assert_eq!(summary_bar_height(&ui), SUMMARY_BAR_HEIGHT);
        ui.process_search = None;
        ui.show_interrupt_prompt(false);
        assert_eq!(summary_bar_height(&ui), SUMMARY_BAR_HEIGHT);
    }

    #[test]
    fn configured_statusline_keeps_contextual_process_actions() {
        let mut model = ActivityModel::default();
        model.apply_activity_event(ActivityEvent::Process(Process::Start {
            id: 1,
            name: "api".to_string(),
            parent: None,
            command: None,
            ports: vec![],
            urls: Box::default(),
            ready_probe: None,
            level: ActivityLevel::Info,
            timestamp: Timestamp::now(),
        }));
        let mut ctx = summary_ctx(false, false, 1);
        ctx.preferences = Arc::new(crate::config::TuiPreferences {
            theme: crate::config::ThemeConfig {
                preset: crate::config::ThemePreset::Terminal,
                ..crate::config::ThemeConfig::default()
            },
            ..crate::config::TuiPreferences::default()
        });
        ctx.keymap = Arc::new(ctx.preferences.keybindings.resolve().unwrap());
        ctx.selected = model.get_activity(1).cloned();
        ctx.selected_has_logs = true;
        ctx.process_search_available = true;

        let output = build_summary_view_impl(&ctx, 400)
            .render(Some(400))
            .to_string();
        for hint in [
            "Ctrl+E logs",
            "Ctrl+X stop",
            "Ctrl+R restart",
            "Esc cancel",
            "Ctrl+H toggle stopped",
            "/ search",
        ] {
            assert!(output.contains(hint), "missing {hint:?}: {output:?}");
        }

        for width in [80, 120, 200] {
            let output = build_summary_view_impl(&ctx, width)
                .render(Some(width as usize))
                .to_string();
            for key in ["^E", "^X", "^R", "Esc", "^H", "/"] {
                assert!(
                    output.contains(key),
                    "missing {key:?} at width {width}: {output:?}"
                );
            }
        }
    }

    #[test]
    fn configured_statusline_does_not_restore_unbound_key_hints() {
        let mut ctx = summary_ctx(false, false, 0);
        let mut preferences = crate::config::TuiPreferences::default();
        for action in ["move_down", "move_up", "half_page_down", "half_page_up"] {
            preferences
                .keybindings
                .main
                .insert(action.to_string(), Vec::new());
        }
        ctx.keymap = Arc::new(preferences.keybindings.resolve().unwrap());
        ctx.preferences = Arc::new(preferences);

        let output = build_summary_view_impl(&ctx, 160)
            .render(Some(160))
            .to_string();
        assert!(!output.contains("navigate"));
        assert!(!output.contains("↑↓/jk"));
        assert!(!output.contains("Ctrl-D/U"));
    }

    #[test]
    fn test_summary_help_reflects_hide_stopped_processes_state() {
        let hidden_output = build_summary_view_impl(&summary_ctx(true, false, 0), 100)
            .render(Some(100))
            .to_string();

        assert!(hidden_output.contains("show stopped"));
        assert!(!hidden_output.contains("hide stopped"));

        let shown_output = build_summary_view_impl(&summary_ctx(false, false, 1), 100)
            .render(Some(100))
            .to_string();

        assert!(shown_output.contains("hide stopped"));
        assert!(!shown_output.contains("show stopped"));
    }

    #[test]
    fn test_summary_help_omits_hide_toggle_when_no_stopped_processes() {
        let output = build_summary_view_impl(&summary_ctx(false, false, 0), 100)
            .render(Some(100))
            .to_string();

        assert!(!output.contains("hide stopped"));
        assert!(!output.contains("show stopped"));
        assert!(!output.contains("Ctrl-H"));
    }

    #[test]
    fn test_summary_navigation_shows_arrow_and_vim_keys() {
        let mut ctx = summary_ctx(false, false, 0);
        ctx.can_go_down = true;

        for width in [48, 100, 180] {
            let output = build_summary_view_impl(&ctx, width)
                .render(Some(width as usize))
                .to_string();
            assert!(
                output.contains("↑↓ j/k"),
                "missing navigation keys at {width}: {output:?}"
            );
            if width < 160 {
                assert!(output.contains("^D/^U"));
            } else {
                assert!(output.contains("Ctrl-D/Ctrl-U"));
            }
        }

        let mut model = ActivityModel::default();
        model.apply_activity_event(ActivityEvent::Process(Process::Start {
            id: 1,
            name: "api".to_string(),
            parent: None,
            command: None,
            ports: vec![],
            urls: Box::default(),
            ready_probe: None,
            level: ActivityLevel::Info,
            timestamp: Timestamp::now(),
        }));
        ctx.selected = model.get_activity(1).cloned();
        ctx.can_go_up = true;
        ctx.summary.running_processes = 1;
        ctx.summary.total_processes = 1;

        let output = build_summary_view_impl(&ctx, 120)
            .render(Some(120))
            .to_string();
        assert!(output.contains("↑↓ j/k ^D/^U nav"));
        assert!(output.contains("1 process"));
    }

    #[test]
    fn test_summary_process_preview_action_matches_scope() {
        let mut model = ActivityModel::default();
        model.apply_activity_event(ActivityEvent::Process(Process::Start {
            id: 1,
            name: "api".to_string(),
            parent: None,
            command: None,
            ports: vec![],
            urls: Box::default(),
            ready_probe: None,
            level: ActivityLevel::Info,
            timestamp: Timestamp::now(),
        }));

        let mut ctx = summary_ctx(false, false, 0);
        ctx.selected = model.get_activity(1).cloned();
        ctx.showing_logs = true;
        ctx.process_previews_fit = true;

        let automatic = build_summary_view_impl(&ctx, 120)
            .render(Some(120))
            .to_string();
        assert!(automatic.contains("hide previews"));

        ctx.selected_preview_focused = true;
        let focused = build_summary_view_impl(&ctx, 120)
            .render(Some(120))
            .to_string();
        assert!(focused.contains("hide preview"));
        assert!(!focused.contains("hide previews"));
    }

    #[test]
    fn test_process_search_keeps_query_visible_across_terminal_widths() {
        let mut ctx = summary_ctx(false, false, 0);
        ctx.process_search = Some("worker".to_string());
        ctx.process_search_matches = 1;

        for width in [32, 48, 72, 120] {
            let output = build_summary_view_impl(&ctx, width)
                .render(Some(width as usize))
                .to_string();

            assert!(
                output.contains("Search processes: /worker"),
                "search query was truncated at {width} columns: {output:?}"
            );
        }

        let output = build_summary_view_impl(&ctx, 120)
            .render(Some(120))
            .to_string();
        assert!(output.contains("1 match"));
        assert!(output.contains("Esc cancel"));
    }

    #[test]
    fn test_process_search_prompt_turns_red_without_matches() {
        assert_eq!(process_search_prompt_color(0), COLOR_FAILED);
        assert_eq!(process_search_prompt_color(1), COLOR_INTERACTIVE);
    }

    #[test]
    fn test_process_search_dims_only_non_matching_processes() {
        let mut model = ActivityModel::default();
        let ui_state = UiState::new();

        for (id, name) in [(1, "email-worker"), (2, "event-router")] {
            model.apply_activity_event(ActivityEvent::Process(Process::Start {
                id,
                name: name.to_string(),
                parent: None,
                command: None,
                ports: vec![],
                urls: Box::default(),
                ready_probe: None,
                level: ActivityLevel::Info,
                timestamp: Timestamp::now(),
            }));
        }

        let matches: HashSet<_> = model
            .get_matching_process_activity_ids(&ui_state, "ema")
            .into_iter()
            .collect();

        assert!(!activity_is_dimmed(
            model.get_activity(1).unwrap(),
            Some(&matches)
        ));
        assert!(activity_is_dimmed(
            model.get_activity(2).unwrap(),
            Some(&matches)
        ));
        assert!(!activity_is_dimmed(model.get_activity(2).unwrap(), None));
    }

    #[test]
    fn test_view_uses_current_ui_state_for_process_visibility() {
        let mut model = ActivityModel::default();
        let mut ui_state = UiState::new();

        model.apply_activity_event(ActivityEvent::Operation(Operation::Start {
            id: 100,
            name: "Running processes".to_string(),
            parent: None,
            detail: None,
            level: ActivityLevel::Info,
            timestamp: Timestamp::now(),
        }));

        model.apply_activity_event(ActivityEvent::Process(Process::Start {
            id: 1,
            name: "manually-stopped".to_string(),
            parent: Some(100),
            command: None,
            ports: vec![],
            urls: Box::default(),
            ready_probe: None,
            level: ActivityLevel::Info,
            timestamp: Timestamp::now(),
        }));
        model.apply_activity_event(ActivityEvent::Process(Process::Status {
            id: 1,
            status: ProcessStatus::Stopped,
            timestamp: Timestamp::now(),
        }));

        let stale_display = model.get_display_activities(&ui_state);
        assert!(
            stale_display
                .iter()
                .any(|da| da.activity.name == "manually-stopped")
        );

        ui_state.hide_stopped_processes = true;

        let display_activities = model.get_display_activities(&ui_state);
        let mut element: AnyElement<'static> = view(
            &model,
            &ui_state,
            RenderContext::Normal,
            Some(ScrollState {
                handle: None,
                display_activities,
                process_previews_fit: false,
            }),
            false,
        )
        .into();
        let rendered = element
            .render(Some(ui_state.terminal_size.width as usize))
            .to_string();

        assert!(!rendered.contains("manually-stopped"));
    }

    #[test]
    fn test_running_processes_label_shows_hidden_count_when_filter_is_active() {
        let mut model = ActivityModel::default();
        let mut ui_state = UiState::new();

        model.apply_activity_event(ActivityEvent::Operation(Operation::Start {
            id: 100,
            name: "Running processes".to_string(),
            parent: None,
            detail: None,
            level: ActivityLevel::Info,
            timestamp: Timestamp::now(),
        }));

        for (id, name) in [(1, "stopped-a"), (2, "stopped-b"), (3, "running")] {
            model.apply_activity_event(ActivityEvent::Process(Process::Start {
                id,
                name: name.to_string(),
                parent: Some(100),
                command: None,
                ports: vec![],
                urls: Box::default(),
                ready_probe: None,
                level: ActivityLevel::Info,
                timestamp: Timestamp::now(),
            }));
        }
        for id in [1, 2] {
            model.apply_activity_event(ActivityEvent::Process(Process::Status {
                id,
                status: ProcessStatus::Stopped,
                timestamp: Timestamp::now(),
            }));
        }

        let render = |ui: &UiState| {
            let display_activities = model.get_display_activities(ui);
            let mut element: AnyElement<'static> = view(
                &model,
                ui,
                RenderContext::Normal,
                Some(ScrollState {
                    handle: None,
                    display_activities,
                    process_previews_fit: false,
                }),
                false,
            )
            .into();
            element
                .render(Some(ui.terminal_size.width as usize))
                .to_string()
        };

        let rendered_visible = render(&ui_state);
        assert!(
            !rendered_visible.contains("hidden)"),
            "no hidden count is shown while the filter is off: {rendered_visible}"
        );

        ui_state.hide_stopped_processes = true;
        let rendered_hidden = render(&ui_state);
        assert!(
            rendered_hidden.contains("(2 hidden)"),
            "hidden count should appear next to the Running processes label: {rendered_hidden}"
        );
    }

    #[test]
    fn statusline_position_controls_terminal_placement() {
        let mut model = ActivityModel::default();
        model.apply_activity_event(ActivityEvent::Process(Process::Start {
            id: 1,
            name: "api".to_string(),
            parent: None,
            command: None,
            ports: vec![],
            urls: Box::default(),
            ready_probe: None,
            level: ActivityLevel::Info,
            timestamp: Timestamp::now(),
        }));
        model.apply_activity_event(process_log(1, "ready", false));

        let statusline_row = |position: StatuslinePosition, open: bool| {
            let mut ui_state = UiState::new();
            ui_state.set_terminal_size(100, 12);
            let mut preferences = crate::config::TuiPreferences::default();
            preferences.statusline.position = position;
            ui_state.set_preferences(preferences).unwrap();
            ui_state.process_previews_hidden = !open;
            ui_state.inline_logs_activity = open.then_some(1);

            let display_activities = model.get_display_activities(&ui_state);
            let child: AnyElement<'static> = view(
                &model,
                &ui_state,
                RenderContext::Normal,
                Some(ScrollState {
                    handle: None,
                    display_activities,
                    process_previews_fit: false,
                }),
                false,
            )
            .into();
            let mut element: AnyElement<'static> = element! {
                View(
                    width: ui_state.terminal_size.width,
                    height: ui_state.terminal_size.height
                ) {
                    #(vec![child])
                }
            }
            .into();
            let output = element
                .render(Some(ui_state.terminal_size.width as usize))
                .to_string();
            let row = output
                .lines()
                .position(|line| line.contains("1 process"))
                .unwrap();
            (row, output)
        };

        for open in [false, true] {
            let (bottom_row, bottom_output) = statusline_row(StatuslinePosition::Bottom, open);
            assert_eq!(bottom_row, 11, "{bottom_output:?}");
            let (top_row, top_output) = statusline_row(StatuslinePosition::Top, open);
            assert_eq!(top_row, 0, "{top_output:?}");
        }

        let (collapsed_row, collapsed_output) = statusline_row(StatuslinePosition::Inline, false);
        let (open_row, open_output) = statusline_row(StatuslinePosition::Inline, true);
        assert_eq!(collapsed_row, 2, "{collapsed_output:?}");
        assert_eq!(open_row, 3, "{open_output:?}");
    }

    #[test]
    fn test_summary_bar_shows_running_of_total_when_some_are_stopped() {
        let mut model = ActivityModel::default();
        let mut ui_state = UiState::new();
        // Wide enough to fit stats + help without column truncation.
        ui_state.set_terminal_size(120, 24);

        model.apply_activity_event(ActivityEvent::Operation(Operation::Start {
            id: 100,
            name: "Running processes".to_string(),
            parent: None,
            detail: None,
            level: ActivityLevel::Info,
            timestamp: Timestamp::now(),
        }));

        for (id, name) in [(1, "stopped"), (2, "running-a"), (3, "running-b")] {
            model.apply_activity_event(ActivityEvent::Process(Process::Start {
                id,
                name: name.to_string(),
                parent: Some(100),
                command: None,
                ports: vec![],
                urls: Box::default(),
                ready_probe: None,
                level: ActivityLevel::Info,
                timestamp: Timestamp::now(),
            }));
        }
        model.apply_activity_event(ActivityEvent::Process(Process::Status {
            id: 1,
            status: ProcessStatus::Stopped,
            timestamp: Timestamp::now(),
        }));

        let display_activities = model.get_display_activities(&ui_state);
        let mut element: AnyElement<'static> = view(
            &model,
            &ui_state,
            RenderContext::Normal,
            Some(ScrollState {
                handle: None,
                display_activities,
                process_previews_fit: false,
            }),
            false,
        )
        .into();
        let rendered = element
            .render(Some(ui_state.terminal_size.width as usize))
            .to_string();

        assert!(
            rendered.contains("2 of 3"),
            "summary bar must surface running-of-total when some are stopped: {rendered}"
        );
    }

    #[test]
    fn test_summary_bar_excludes_not_started_processes_from_total() {
        let mut model = ActivityModel::default();
        let mut ui_state = UiState::new();
        ui_state.set_terminal_size(120, 24);

        model.apply_activity_event(ActivityEvent::Operation(Operation::Start {
            id: 100,
            name: "Running processes".to_string(),
            parent: None,
            detail: None,
            level: ActivityLevel::Info,
            timestamp: Timestamp::now(),
        }));

        for (id, name) in [(1, "disabled-a"), (2, "disabled-b"), (3, "running")] {
            model.apply_activity_event(ActivityEvent::Process(Process::Start {
                id,
                name: name.to_string(),
                parent: Some(100),
                command: None,
                ports: vec![],
                urls: Box::default(),
                ready_probe: None,
                level: ActivityLevel::Info,
                timestamp: Timestamp::now(),
            }));
        }
        for id in [1, 2] {
            model.apply_activity_event(ActivityEvent::Process(Process::Status {
                id,
                status: ProcessStatus::NotStarted,
                timestamp: Timestamp::now(),
            }));
        }

        let summary = model.calculate_summary();
        assert_eq!(summary.running_processes, 1);
        assert_eq!(summary.stopped_processes, 0);
        assert_eq!(
            summary.total_processes, 1,
            "NotStarted processes must not inflate the tracked total"
        );

        let display_activities = model.get_display_activities(&ui_state);
        let mut element: AnyElement<'static> = view(
            &model,
            &ui_state,
            RenderContext::Normal,
            Some(ScrollState {
                handle: None,
                display_activities,
                process_previews_fit: false,
            }),
            false,
        )
        .into();
        let rendered = element
            .render(Some(ui_state.terminal_size.width as usize))
            .to_string();

        assert!(
            !rendered.contains("of 1 processes") && !rendered.contains("0 of"),
            "bar must render `1 process` not `0 of 1`: {rendered}"
        );
        assert!(
            rendered.contains("1 process"),
            "running count should still surface: {rendered}"
        );
    }

    #[test]
    fn process_view_renders_every_status_label_and_gave_up_affordance() {
        let mut model = ActivityModel::default();
        let mut ui_state = UiState::new();
        ui_state.set_terminal_size(200, 30);

        model.apply_activity_event(ActivityEvent::Operation(Operation::Start {
            id: 100,
            name: "Running processes".to_string(),
            parent: None,
            detail: None,
            level: ActivityLevel::Info,
            timestamp: Timestamp::now(),
        }));

        for (offset, (name, status)) in [
            ("not-started", ProcessStatus::NotStarted),
            ("waiting", ProcessStatus::Waiting),
            ("starting", ProcessStatus::Starting),
            ("running", ProcessStatus::Running),
            ("ready", ProcessStatus::Ready),
            ("restarting", ProcessStatus::Restarting),
            ("stopping", ProcessStatus::Stopping),
            ("stopped", ProcessStatus::Stopped),
            ("exited", ProcessStatus::Exited),
            ("gave-up", ProcessStatus::GaveUp),
        ]
        .into_iter()
        .enumerate()
        {
            let id = offset as u64 + 1;
            model.apply_activity_event(ActivityEvent::Process(Process::Start {
                id,
                name: name.to_string(),
                parent: Some(100),
                command: None,
                ports: vec![],
                urls: Box::default(),
                ready_probe: None,
                level: ActivityLevel::Info,
                timestamp: Timestamp::now(),
            }));
            model.apply_activity_event(ActivityEvent::Process(Process::Status {
                id,
                status,
                timestamp: Timestamp::now(),
            }));
        }
        ui_state.selected_activity = Some(10);

        let display_activities = model.get_display_activities(&ui_state);
        let mut element: AnyElement<'static> = view(
            &model,
            &ui_state,
            RenderContext::Normal,
            Some(ScrollState {
                handle: None,
                display_activities,
                process_previews_fit: false,
            }),
            false,
        )
        .into();
        let rendered = element.render(Some(200)).to_string();

        for label in [
            "auto start off",
            "waiting",
            "starting",
            "running",
            "ready",
            "restarting",
            "stopping",
            "stopped",
            "exited",
            "gave up (crash loop)",
        ] {
            assert!(
                rendered.contains(label),
                "missing process label {label:?}: {rendered}"
            );
        }
        assert!(
            rendered.contains("(re)start process"),
            "GaveUp must remain restartable: {rendered}"
        );
        assert!(
            !rendered.contains("stop process"),
            "GaveUp is terminal and must not offer stop: {rendered}"
        );
    }
}
