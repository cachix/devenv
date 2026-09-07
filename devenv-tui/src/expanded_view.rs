//! Expanded log view component.
//!
//! This component displays build logs in a fullscreen view using the alternate screen buffer.
//! It provides scrollable access to all log lines for a selected activity.
//! Scroll offset is managed as component-local state for immediate responsiveness.
//! Supports mouse-based text selection with OSC 52 clipboard copy.

use crate::TuiConfig;
use crate::app::{
    ExitFlag, PauseFlag, ProcessCommandSender, handle_interrupt_prompt_action,
    request_interrupt_prompt,
};
use crate::components::{COLOR_COMPLETED, COLOR_INTERACTIVE};
use crate::config::{Action, KeyContext, KeyMatch, KeySequenceState, StatuslinePosition};
use crate::model::{ActivityModel, UiState, ViewMode};
use crate::statusline::{
    StatuslineData, StatuslineMode, action_key_hints, interrupt_prompt_key_hints, render_statusline,
};
use base64::Engine;
use crossterm::event::MouseButton;
use human_repr::HumanCount;
use iocraft::prelude::*;
use iocraft::{FullscreenMouseEvent, MouseEventKind};
use std::collections::VecDeque;
use std::io::Write as _;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::Instant;
use tokio_shutdown::Shutdown;

/// Width of the line-number field, e.g. "NNNNN".
const LINE_NUM_DIGITS: usize = 5;
/// Separator between line number and content. Three terminal columns wide.
const LINE_NUM_SEPARATOR: &str = " \u{2502} ";
const LINE_NUM_SEPARATOR_WIDTH: usize = 3;
/// Full prefix width, e.g. "NNNNN │ " = 8 columns.
const LINE_NUM_PREFIX_WIDTH: usize = LINE_NUM_DIGITS + LINE_NUM_SEPARATOR_WIDTH;
const COPY_NOTICE_DURATION: Duration = Duration::from_secs(2);

/// Collect `s.chars()[start..end]` into a new `String`.
fn char_slice(s: &str, start: usize, end: usize) -> String {
    s.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

/// Represents a normalized text selection range.
struct Selection {
    /// (log_line_index, visual_col), always <= end
    start: (usize, usize),
    /// (log_line_index, visual_col), always >= start
    end: (usize, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScrollAnchor {
    log_line: usize,
    char_start: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScrollMode {
    follow_tail: bool,
    anchor: Option<ScrollAnchor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogScrollAction {
    Forward(usize),
    Backward(usize),
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LogSearchCursor {
    log_line: usize,
    char_start: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LogSearchOrigin {
    offset: usize,
    scroll_mode: ScrollMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LogSearchState {
    query: String,
    current: Option<LogSearchCursor>,
    editing: bool,
    origin: LogSearchOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LogSearchMatch {
    log_line: usize,
    char_start: usize,
    char_end: usize,
}

#[derive(Clone, Copy)]
struct LogSearchView<'a> {
    query: &'a str,
    matches: &'a [LogSearchMatch],
    current: Option<LogSearchCursor>,
    editing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CopyNotice {
    lines: usize,
    bytes: usize,
}

struct ExpandedViewUi<'a> {
    selection: Option<&'a Selection>,
    search: Option<LogSearchView<'a>>,
    copy_notice: Option<CopyNotice>,
    scroll_mode: ScrollMode,
    interrupt_prompt: (bool, bool),
}

struct ExpandedCustomization {
    preferences: Arc<crate::config::TuiPreferences>,
    keymap: Arc<crate::config::Keymap>,
    context: Arc<crate::config::TuiRunContext>,
    pending_key: Option<String>,
}

impl Selection {
    /// Create a selection from anchor and cursor, normalizing so start <= end.
    fn from_anchor_cursor(anchor: (usize, usize), cursor: (usize, usize)) -> Self {
        if anchor.0 < cursor.0 || (anchor.0 == cursor.0 && anchor.1 <= cursor.1) {
            Selection {
                start: anchor,
                end: cursor,
            }
        } else {
            Selection {
                start: cursor,
                end: anchor,
            }
        }
    }

    /// Returns the selected column range for a given line, if it overlaps.
    /// Returns (start_col, end_col) where end_col is exclusive.
    fn line_range(&self, line_idx: usize, line_len: usize) -> Option<(usize, usize)> {
        if line_idx < self.start.0 || line_idx > self.end.0 {
            return None;
        }
        let start_col = if line_idx == self.start.0 {
            self.start.1
        } else {
            0
        };
        let end_col = if line_idx == self.end.0 {
            self.end.1
        } else {
            line_len
        };
        if start_col >= end_col && line_idx == self.start.0 && line_idx == self.end.0 {
            return None;
        }
        Some((start_col.min(line_len), end_col.min(line_len)))
    }
}

/// Fullscreen component for viewing expanded logs.
///
/// This component runs in fullscreen mode (alternate screen buffer) to avoid
/// affecting terminal scrollback. It provides vim-like navigation for scrolling
/// through log content, mouse-based text selection, and OSC 52 clipboard copy.
///
/// Scroll offset is managed as component-local state for immediate responsiveness -
/// no model locks are acquired during keyboard/mouse event handling.
#[component]
pub fn ExpandedLogView(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let config = hooks.use_context::<Arc<TuiConfig>>();
    let activity_model = hooks.use_context::<Arc<RwLock<ActivityModel>>>();
    let ui_state = hooks.use_context::<Arc<RwLock<UiState>>>();
    let activity_id = *hooks.use_context::<u64>();
    let notify = hooks.use_context::<Arc<Notify>>();
    let model_version = hooks.use_context::<crate::app::ModelVersion>().0.clone();
    let render_shutdown = hooks.use_context::<crate::app::RenderShutdown>().0.clone();
    let shutdown = hooks.use_context::<Arc<Shutdown>>();
    let command_tx = hooks.use_context::<Option<ProcessCommandSender>>();
    let (width, height) = hooks.use_terminal_size();

    // Component-local scroll state - updates are immediate, no model lock needed.
    // The offset is measured in visual rows (one per terminal row); a single
    // log line may span multiple visual rows when wrapped.
    let mut scroll_offset = hooks.use_state(|| 0usize);
    let default_follow = ui_state
        .read()
        .map(|ui| ui.preferences.behavior.follow_logs)
        .unwrap_or(true);
    let mut follow_tail = hooks.use_state(|| default_follow);
    let mut scroll_anchor = hooks.use_state(|| None::<ScrollAnchor>);
    let mut log_search = hooks.use_state(|| None::<LogSearchState>);
    let mut copy_notice = hooks.use_state(|| None::<CopyNotice>);
    let mut copy_notice_deadline = hooks.use_ref(|| None::<Instant>);
    let copy_notice_wake = hooks.use_ref(|| Arc::new(Notify::new()));

    let copy_notice_wake_for_timer = copy_notice_wake.read().clone();
    hooks.use_future(async move {
        loop {
            copy_notice_wake_for_timer.notified().await;
            while let Some(Some(deadline)) = copy_notice_deadline.try_get() {
                tokio::time::sleep_until(deadline).await;
                let Some(latest_deadline) = copy_notice_deadline.try_get() else {
                    return;
                };
                if latest_deadline.is_some_and(|latest| latest > Instant::now()) {
                    continue;
                }
                copy_notice.set(None);
                copy_notice_deadline.set(None);
                break;
            }
        }
    });

    // Selection state. Coordinates are (log_line_idx, visual_col) where
    // visual_col is a logical character offset within the log line, regardless
    // of how it wraps onto multiple visual rows.
    let mut selection_anchor = hooks.use_state(|| None::<(usize, usize)>);
    let mut selection_cursor = hooks.use_state(|| None::<(usize, usize)>);
    let mut is_selecting = hooks.use_state(|| false);
    let keymap = ui_state.read().ok().map(|ui| ui.keymap().clone());
    let key_sequence = hooks.use_state(KeySequenceState::default);
    let key_sequence_wake = hooks.use_ref(|| Arc::new(Notify::new()));
    let key_sequence_wake_for_timer = key_sequence_wake.read().clone();
    hooks.use_future({
        let ui_state = ui_state.clone();
        let notify = notify.clone();
        let mut key_sequence = key_sequence;
        async move {
            loop {
                key_sequence_wake_for_timer.notified().await;
                loop {
                    let remaining = key_sequence.read().remaining_timeout();
                    let Some(remaining) = remaining else {
                        break;
                    };
                    tokio::select! {
                        _ = tokio::time::sleep(remaining) => {
                            if key_sequence.write().expire() {
                                if let Ok(mut ui) = ui_state.write() {
                                    ui.pending_key = None;
                                }
                                notify.notify_one();
                            }
                            break;
                        }
                        _ = key_sequence_wake_for_timer.notified() => {}
                    }
                }
            }
        }
    });

    // Redraw when notified of activity model changes (throttled)
    // This handles new log lines being added
    let redraw = hooks.use_state(|| 0u64);
    hooks.use_future({
        let notify = notify.clone();
        let model_version = model_version.clone();
        let render_shutdown = render_shutdown.clone();
        let max_fps = config.max_fps;
        async move {
            crate::throttled_notify_loop(notify, render_shutdown, model_version, redraw, max_fps)
                .await;
        }
    });

    // Extract view state from activity model (read-only, brief lock)
    let view_state = {
        let model_guard = activity_model.read().unwrap();
        extract_view_state(&model_guard, activity_id, scroll_offset.get())
    };

    let Some(state) = view_state else {
        // Activity not found, exit
        if let Ok(mut ui) = ui_state.write() {
            ui.view_mode = ViewMode::Main;
        }
        hooks.use_context_mut::<SystemContext>().exit();
        return element!(View).into_any();
    };

    let content_width = content_width_for(width);
    let (statusline_position, statusline_enabled, prompt_active) = ui_state
        .read()
        .map(|ui| {
            (
                ui.preferences.statusline.position,
                ui.preferences.statusline.enabled,
                ui.interrupt_prompt_active(),
            )
        })
        .unwrap_or((StatuslinePosition::Inline, true, false));
    let footer_visible = statusline_enabled
        || prompt_active
        || log_search.read().is_some()
        || copy_notice.get().is_some();
    let viewport_height = calculate_viewport_height(height, footer_visible);

    // Memoize the wrap layout: rebuilding it on every render walks the entire
    // log buffer, and we'd clone the result again for event handlers. The Arc
    // pointer of `state.logs` is swapped when the buffer changes (Arc::make_mut
    // in handle_activity_log), so (ptr, content_width) is a sufficient key.
    let visual_rows: Arc<Vec<VisualRow>> = hooks.use_memo(
        || Arc::new(build_visual_rows(&state.logs, content_width)),
        (Arc::as_ptr(&state.logs) as usize, content_width),
    );
    let search_query = log_search
        .read()
        .as_ref()
        .map(|search| search.query.clone())
        .unwrap_or_default();
    let search_matches: Arc<Vec<LogSearchMatch>> = hooks.use_memo(
        || {
            Arc::new(find_log_matches(
                &state.logs,
                state.buffer_start_line,
                &search_query,
            ))
        },
        (
            Arc::as_ptr(&state.logs) as usize,
            state.buffer_start_line,
            search_query.clone(),
        ),
    );
    let total_visual_rows = visual_rows.len();
    let current_selection_anchor = selection_anchor.get();
    let current_selection_cursor = selection_cursor.get();
    let has_selection = current_selection_anchor.is_some() && current_selection_cursor.is_some();

    let logs_for_copy = state.logs.clone();
    let visual_rows_for_events = visual_rows.clone();
    let copy_notice_wake_for_events = copy_notice_wake.read().clone();

    // Handle keyboard and mouse events - NO MODEL LOCK, only local state updates
    hooks.use_terminal_events({
        let ui_state = ui_state.clone();
        let shutdown = shutdown.clone();
        let command_tx = command_tx.clone();
        let notify = notify.clone();
        let attached = config.attached.load(std::sync::atomic::Ordering::Relaxed);
        let keymap = keymap.clone();
        let mut key_sequence = key_sequence;
        let key_sequence_wake = key_sequence_wake.read().clone();
        let mouse_enabled = ui_state
            .read()
            .map(|ui| ui.preferences.behavior.mouse)
            .unwrap_or(true);
        move |event| match event {
            TerminalEvent::Key(key_event) => {
                if key_event.kind == KeyEventKind::Release {
                    return;
                }
                let prompt_active = ui_state
                    .read()
                    .map(|ui| ui.interrupt_prompt_active())
                    .unwrap_or(false);
                let search_editing = log_search
                    .read()
                    .as_ref()
                    .is_some_and(|search| search.editing);
                let context = if prompt_active {
                    KeyContext::Prompt
                } else if search_editing {
                    KeyContext::LogSearch
                } else {
                    KeyContext::Logs
                };
                let emergency_interrupt =
                    crate::config::is_emergency_interrupt(key_event.code, key_event.modifiers);
                let (key_match, pending_key) = {
                    let mut sequence = key_sequence.write();
                    let key_match = if emergency_interrupt {
                        sequence.clear();
                        KeyMatch::None
                    } else if let Some(keymap) = keymap.as_deref() {
                        sequence.input_key(keymap, context, key_event.code, key_event.modifiers)
                    } else {
                        KeyMatch::None
                    };
                    (key_match, sequence.pending_label())
                };
                key_sequence_wake.notify_one();
                if let Ok(mut ui) = ui_state.write() {
                    ui.pending_key = pending_key;
                }
                let action = match key_match {
                    KeyMatch::Action(action) => Some(action),
                    KeyMatch::Prefix | KeyMatch::None => None,
                };
                handle_key_event(
                    key_event,
                    action,
                    key_match != KeyMatch::None,
                    emergency_interrupt,
                    &ui_state,
                    &shutdown,
                    command_tx.as_ref(),
                    attached,
                    &mut scroll_offset,
                    &mut follow_tail,
                    &mut scroll_anchor,
                    state.buffer_start_line,
                    &visual_rows_for_events,
                    total_visual_rows,
                    viewport_height,
                    &mut log_search,
                    &mut CopyFeedbackState {
                        notice: &mut copy_notice,
                        deadline: &mut copy_notice_deadline,
                        wake: &copy_notice_wake_for_events,
                    },
                    &mut SelectionState {
                        has_selection,
                        anchor: current_selection_anchor,
                        cursor: current_selection_cursor,
                        logs: &logs_for_copy,
                        buffer_start_line: state.buffer_start_line,
                        anchor_state: &mut selection_anchor,
                        cursor_state: &mut selection_cursor,
                        is_selecting: &mut is_selecting,
                    },
                );
                // Exit keys flip `view_mode` on `ui_state`, which iocraft can't
                // observe; wake the render loop so leaving the view is prompt
                // instead of waiting for the idle heartbeat (#2915).
                notify.notify_one();
            }
            TerminalEvent::FullscreenMouse(mouse_event) => {
                let prompt_active = ui_state
                    .read()
                    .map(|ui| ui.interrupt_prompt_active())
                    .unwrap_or(false);
                if mouse_enabled && !prompt_active {
                    handle_mouse_event(
                        mouse_event,
                        &mut scroll_offset,
                        &mut follow_tail,
                        &mut scroll_anchor,
                        state.buffer_start_line,
                        total_visual_rows,
                        viewport_height,
                        statusline_position,
                        footer_visible,
                        &visual_rows_for_events,
                        &mut selection_anchor,
                        &mut selection_cursor,
                        &mut is_selecting,
                    );
                }
            }
            TerminalEvent::Resize(_, _) | _ => {}
        }
    });

    // Check if we should exit (backend done or view mode changed)
    let exit_flag = hooks.use_context::<ExitFlag>();
    let pause_flag = hooks.use_context::<PauseFlag>();
    let should_exit = exit_flag.is_set()
        || pause_flag.is_set()
        || ui_state
            .read()
            .map(|ui| !matches!(ui.view_mode, ViewMode::ExpandedLogs { .. }))
            .unwrap_or(false);
    if should_exit {
        hooks.use_context_mut::<SystemContext>().exit();
        return element!(View).into_any();
    }

    // Build selection for rendering
    let selection =
        if let (Some(anchor), Some(cursor)) = (selection_anchor.get(), selection_cursor.get()) {
            Some(Selection::from_anchor_cursor(anchor, cursor))
        } else {
            None
        };

    let (interrupt_prompt_active, interrupt_prompt_attached) = ui_state
        .read()
        .map(|ui| (ui.interrupt_prompt_active(), ui.interrupt_prompt_attached()))
        .unwrap_or((false, false));
    let search_state = log_search.read().clone();
    let search = search_state.as_ref().map(|search| LogSearchView {
        query: &search.query,
        matches: &search_matches,
        current: search.current,
        editing: search.editing,
    });

    let customization = ui_state.read().ok().map(|ui| ExpandedCustomization {
        preferences: ui.preferences.clone(),
        keymap: ui.keymap().clone(),
        context: ui.run_context.clone(),
        pending_key: ui.pending_key.clone(),
    });
    render_expanded_view_custom(
        &state,
        &visual_rows,
        width,
        height,
        ExpandedViewUi {
            selection: selection.as_ref(),
            search,
            copy_notice: copy_notice.get(),
            scroll_mode: ScrollMode {
                follow_tail: follow_tail.get(),
                anchor: scroll_anchor.get(),
            },
            interrupt_prompt: (interrupt_prompt_active, interrupt_prompt_attached),
        },
        customization.as_ref(),
    )
}

/// State extracted from the model for rendering
struct ExpandedViewState {
    activity_name: String,
    scroll_offset: usize,
    logs: Arc<VecDeque<String>>,
    buffer_start_line: usize,
}

/// A single visual row to render: a slice of one log line, defined by the log
/// line index and the half-open character range `[char_start, char_end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VisualRow {
    log_idx: usize,
    char_start: usize,
    char_end: usize,
}

/// Extract the view state from the activity model
fn extract_view_state(
    model: &ActivityModel,
    activity_id: u64,
    scroll_offset: usize,
) -> Option<ExpandedViewState> {
    let activity_name = model
        .get_activity(activity_id)
        .map(|a| a.name.clone())
        .unwrap_or_else(|| format!("Activity {}", activity_id));

    // Clone the Arc, not the data - this is cheap
    let logs = model
        .get_build_logs(activity_id)
        .cloned()
        .unwrap_or_else(|| Arc::new(VecDeque::new()));
    let buffer_start_line = model
        .get_log_line_count(activity_id)
        .saturating_sub(logs.len());

    Some(ExpandedViewState {
        activity_name,
        scroll_offset,
        logs,
        buffer_start_line,
    })
}

/// Width available for log content after the line-number gutter.
fn content_width_for(width: u16) -> usize {
    (width as usize).saturating_sub(LINE_NUM_PREFIX_WIDTH)
}

/// Build the flat sequence of visual rows for the current logs.
///
/// Each log line maps to one or more visual rows of at most `content_width`
/// characters. Short lines produce a single row; long lines wrap onto
/// continuation rows so their full content stays readable, matching the
/// default behavior of `less`, `journalctl`, and a terminal in line-wrap mode.
fn build_visual_rows(logs: &VecDeque<String>, content_width: usize) -> Vec<VisualRow> {
    let mut rows = Vec::with_capacity(logs.len());
    let width = content_width.max(1);

    for (log_idx, line) in logs.iter().enumerate() {
        let total_chars = line.chars().count();
        if total_chars <= width {
            rows.push(VisualRow {
                log_idx,
                char_start: 0,
                char_end: total_chars,
            });
            continue;
        }

        let mut start = 0;
        while start < total_chars {
            let end = (start + width).min(total_chars);
            rows.push(VisualRow {
                log_idx,
                char_start: start,
                char_end: end,
            });
            start = end;
        }
    }

    rows
}

/// Calculate the viewport height (total height minus header and footer)
fn calculate_viewport_height(terminal_height: u16, footer_visible: bool) -> usize {
    (terminal_height as usize).saturating_sub(1 + usize::from(footer_visible))
}

fn resolved_scroll_offset(
    scroll_offset: usize,
    follow_tail: bool,
    scroll_anchor: Option<ScrollAnchor>,
    buffer_start_line: usize,
    visual_rows: &[VisualRow],
    total_visual_rows: usize,
    viewport_height: usize,
) -> usize {
    let max_offset = total_visual_rows.saturating_sub(viewport_height);
    if follow_tail {
        max_offset
    } else if let Some(anchor) = scroll_anchor {
        if anchor.log_line < buffer_start_line {
            return 0;
        }
        let relative_line = anchor.log_line - buffer_start_line;
        visual_rows
            .iter()
            .position(|row| {
                row.log_idx > relative_line
                    || row.log_idx == relative_line && row.char_start >= anchor.char_start
            })
            .unwrap_or(max_offset)
            .min(max_offset)
    } else {
        scroll_offset.min(max_offset)
    }
}

fn scroll_anchor_for_offset(
    buffer_start_line: usize,
    visual_rows: &[VisualRow],
    offset: usize,
) -> Option<ScrollAnchor> {
    visual_rows.get(offset).map(|row| ScrollAnchor {
        log_line: buffer_start_line + row.log_idx,
        char_start: row.char_start,
    })
}

fn pause_at_offset(
    scroll_offset: &mut State<usize>,
    follow_tail: &mut State<bool>,
    scroll_anchor: &mut State<Option<ScrollAnchor>>,
    buffer_start_line: usize,
    visual_rows: &[VisualRow],
    offset: usize,
) {
    scroll_offset.set(offset);
    follow_tail.set(false);
    scroll_anchor.set(scroll_anchor_for_offset(
        buffer_start_line,
        visual_rows,
        offset,
    ));
}

fn follow_at_bottom(
    scroll_offset: &mut State<usize>,
    follow_tail: &mut State<bool>,
    scroll_anchor: &mut State<Option<ScrollAnchor>>,
    max_offset: usize,
) {
    scroll_offset.set(max_offset);
    follow_tail.set(true);
    scroll_anchor.set(None);
}

#[cfg(test)]
fn keyboard_scroll_action(key_event: &KeyEvent, viewport_height: usize) -> Option<LogScrollAction> {
    let control = key_event.modifiers.contains(KeyModifiers::CONTROL);
    let half_page = viewport_height.div_ceil(2).max(1);

    match key_event.code {
        KeyCode::Down | KeyCode::Char('j') => Some(LogScrollAction::Forward(1)),
        KeyCode::Up | KeyCode::Char('k') => Some(LogScrollAction::Backward(1)),
        KeyCode::PageDown | KeyCode::Char(' ') => Some(LogScrollAction::Forward(viewport_height)),
        KeyCode::PageUp => Some(LogScrollAction::Backward(viewport_height)),
        KeyCode::Char('d') if control => Some(LogScrollAction::Forward(half_page)),
        KeyCode::Char('u') if control => Some(LogScrollAction::Backward(half_page)),
        KeyCode::Char('f') if control => Some(LogScrollAction::Forward(viewport_height)),
        KeyCode::Char('b') if control => Some(LogScrollAction::Backward(viewport_height)),
        KeyCode::Home | KeyCode::Char('g') => Some(LogScrollAction::Top),
        KeyCode::End | KeyCode::Char('G') => Some(LogScrollAction::Bottom),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_scroll_action(
    action: LogScrollAction,
    current_scroll_offset: usize,
    max_offset: usize,
    scroll_offset: &mut State<usize>,
    follow_tail: &mut State<bool>,
    scroll_anchor: &mut State<Option<ScrollAnchor>>,
    buffer_start_line: usize,
    visual_rows: &[VisualRow],
) {
    match action {
        LogScrollAction::Forward(lines) => {
            let next = current_scroll_offset.saturating_add(lines).min(max_offset);
            if next == max_offset {
                follow_at_bottom(scroll_offset, follow_tail, scroll_anchor, max_offset);
            } else {
                pause_at_offset(
                    scroll_offset,
                    follow_tail,
                    scroll_anchor,
                    buffer_start_line,
                    visual_rows,
                    next,
                );
            }
        }
        LogScrollAction::Backward(lines) => pause_at_offset(
            scroll_offset,
            follow_tail,
            scroll_anchor,
            buffer_start_line,
            visual_rows,
            current_scroll_offset.saturating_sub(lines),
        ),
        LogScrollAction::Top => pause_at_offset(
            scroll_offset,
            follow_tail,
            scroll_anchor,
            buffer_start_line,
            visual_rows,
            0,
        ),
        LogScrollAction::Bottom => {
            follow_at_bottom(scroll_offset, follow_tail, scroll_anchor, max_offset)
        }
    }
}

fn folded_chars(value: &str) -> Vec<char> {
    value.chars().flat_map(char::to_lowercase).collect()
}

fn case_insensitive_match_ranges(line: &str, query: &str) -> Vec<(usize, usize)> {
    let needle = folded_chars(query);
    if needle.is_empty() {
        return vec![];
    }

    let haystack: Vec<_> = line
        .chars()
        .enumerate()
        .flat_map(|(source_index, character)| {
            character
                .to_lowercase()
                .map(move |folded| (folded, source_index))
        })
        .collect();
    let mut ranges = Vec::new();
    let mut index = 0;

    while index + needle.len() <= haystack.len() {
        if haystack[index..index + needle.len()]
            .iter()
            .map(|(character, _)| *character)
            .eq(needle.iter().copied())
        {
            let range = (haystack[index].1, haystack[index + needle.len() - 1].1 + 1);
            if ranges.last().copied() != Some(range) {
                ranges.push(range);
            }
            index += needle.len();
        } else {
            index += 1;
        }
    }

    ranges
}

fn find_log_matches(
    logs: &VecDeque<String>,
    buffer_start_line: usize,
    query: &str,
) -> Vec<LogSearchMatch> {
    logs.iter()
        .enumerate()
        .flat_map(|(log_idx, line)| {
            case_insensitive_match_ranges(line, query).into_iter().map(
                move |(char_start, char_end)| LogSearchMatch {
                    log_line: buffer_start_line + log_idx,
                    char_start,
                    char_end,
                },
            )
        })
        .collect()
}

fn search_cursor(search_match: LogSearchMatch) -> LogSearchCursor {
    LogSearchCursor {
        log_line: search_match.log_line,
        char_start: search_match.char_start,
    }
}

fn choose_search_match(
    matches: &[LogSearchMatch],
    current: Option<LogSearchCursor>,
    current_scroll_offset: usize,
    buffer_start_line: usize,
    visual_rows: &[VisualRow],
) -> Option<LogSearchMatch> {
    if let Some(current) = current
        && let Some(search_match) = matches
            .iter()
            .find(|search_match| search_cursor(**search_match) == current)
    {
        return Some(*search_match);
    }

    let visible_cursor = visual_rows
        .get(current_scroll_offset)
        .map(|row| LogSearchCursor {
            log_line: buffer_start_line + row.log_idx,
            char_start: row.char_start,
        });

    visible_cursor
        .and_then(|cursor| {
            matches.iter().find(|search_match| {
                let candidate = search_cursor(**search_match);
                (candidate.log_line, candidate.char_start) >= (cursor.log_line, cursor.char_start)
            })
        })
        .copied()
        .or_else(|| matches.first().copied())
}

fn adjacent_search_match(
    matches: &[LogSearchMatch],
    current: Option<LogSearchCursor>,
    forward: bool,
) -> Option<LogSearchMatch> {
    if matches.is_empty() {
        return None;
    }

    let index = current.and_then(|current| {
        matches
            .iter()
            .position(|search_match| search_cursor(*search_match) == current)
    });
    let next = match (index, forward) {
        (Some(index), true) => (index + 1) % matches.len(),
        (Some(0), false) | (None, false) => matches.len() - 1,
        (Some(index), false) => index - 1,
        (None, true) => 0,
    };
    matches.get(next).copied()
}

fn visual_offset_for_search_match(
    search_match: LogSearchMatch,
    buffer_start_line: usize,
    visual_rows: &[VisualRow],
) -> Option<usize> {
    let log_idx = search_match.log_line.checked_sub(buffer_start_line)?;
    visual_rows.iter().position(|row| {
        row.log_idx == log_idx
            && row.char_start <= search_match.char_start
            && search_match.char_start < row.char_end.max(row.char_start + 1)
    })
}

#[allow(clippy::too_many_arguments)]
fn focus_search_match(
    search_match: LogSearchMatch,
    viewport_height: usize,
    max_offset: usize,
    scroll_offset: &mut State<usize>,
    follow_tail: &mut State<bool>,
    scroll_anchor: &mut State<Option<ScrollAnchor>>,
    buffer_start_line: usize,
    visual_rows: &[VisualRow],
) {
    if let Some(match_offset) =
        visual_offset_for_search_match(search_match, buffer_start_line, visual_rows)
    {
        let offset = match_offset
            .saturating_sub(viewport_height / 2)
            .min(max_offset);
        pause_at_offset(
            scroll_offset,
            follow_tail,
            scroll_anchor,
            buffer_start_line,
            visual_rows,
            offset,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_log_search_key(
    key_event: &KeyEvent,
    action: Option<Action>,
    key_consumed: bool,
    log_search: &mut State<Option<LogSearchState>>,
    logs: &VecDeque<String>,
    buffer_start_line: usize,
    visual_rows: &[VisualRow],
    current_scroll_offset: usize,
    viewport_height: usize,
    max_offset: usize,
    scroll_offset: &mut State<usize>,
    follow_tail: &mut State<bool>,
    scroll_anchor: &mut State<Option<ScrollAnchor>>,
) -> bool {
    let control = key_event.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key_event.modifiers.contains(KeyModifiers::ALT);
    let current_search = log_search.read().clone();

    if let Some(mut search) = current_search.clone().filter(|search| search.editing) {
        if action == Some(Action::Cancel)
            || crate::config::is_emergency_interrupt(key_event.code, key_event.modifiers)
        {
            scroll_offset.set(search.origin.offset);
            follow_tail.set(search.origin.scroll_mode.follow_tail);
            scroll_anchor.set(search.origin.scroll_mode.anchor);
            log_search.set(None);
            return true;
        }
        match action {
            Some(Action::Accept) => {
                if search.query.is_empty() {
                    scroll_offset.set(search.origin.offset);
                    follow_tail.set(search.origin.scroll_mode.follow_tail);
                    scroll_anchor.set(search.origin.scroll_mode.anchor);
                    log_search.set(None);
                } else {
                    search.editing = false;
                    log_search.set(Some(search));
                }
            }
            None if !key_consumed && key_event.code == KeyCode::Backspace => {
                search.query.pop();
                let matches = find_log_matches(logs, buffer_start_line, &search.query);
                let chosen = choose_search_match(
                    &matches,
                    search.current,
                    current_scroll_offset,
                    buffer_start_line,
                    visual_rows,
                );
                search.current = chosen.map(search_cursor);
                if let Some(chosen) = chosen {
                    focus_search_match(
                        chosen,
                        viewport_height,
                        max_offset,
                        scroll_offset,
                        follow_tail,
                        scroll_anchor,
                        buffer_start_line,
                        visual_rows,
                    );
                }
                log_search.set(Some(search));
            }
            None if !key_consumed
                && matches!(key_event.code, KeyCode::Char(_))
                && !control
                && !alt =>
            {
                let KeyCode::Char(character) = key_event.code else {
                    unreachable!()
                };
                search.query.push(character);
                let matches = find_log_matches(logs, buffer_start_line, &search.query);
                let chosen = choose_search_match(
                    &matches,
                    search.current,
                    current_scroll_offset,
                    buffer_start_line,
                    visual_rows,
                );
                search.current = chosen.map(search_cursor);
                if let Some(chosen) = chosen {
                    focus_search_match(
                        chosen,
                        viewport_height,
                        max_offset,
                        scroll_offset,
                        follow_tail,
                        scroll_anchor,
                        buffer_start_line,
                        visual_rows,
                    );
                }
                log_search.set(Some(search));
            }
            Some(_) | None => {}
        }
        return true;
    }

    if action == Some(Action::Search) {
        log_search.set(Some(LogSearchState {
            query: String::new(),
            current: None,
            editing: true,
            origin: LogSearchOrigin {
                offset: current_scroll_offset,
                scroll_mode: ScrollMode {
                    follow_tail: follow_tail.get(),
                    anchor: scroll_anchor.get(),
                },
            },
        }));
        return true;
    }

    let Some(mut search) = current_search else {
        return false;
    };

    match action {
        Some(Action::Cancel) => {
            log_search.set(None);
            true
        }
        Some(Action::NextMatch | Action::PreviousMatch) => {
            let matches = find_log_matches(logs, buffer_start_line, &search.query);
            let chosen =
                adjacent_search_match(&matches, search.current, action == Some(Action::NextMatch));
            search.current = chosen.map(search_cursor);
            if let Some(chosen) = chosen {
                focus_search_match(
                    chosen,
                    viewport_height,
                    max_offset,
                    scroll_offset,
                    follow_tail,
                    scroll_anchor,
                    buffer_start_line,
                    visual_rows,
                );
            }
            log_search.set(Some(search));
            true
        }
        Some(_) | None => false,
    }
}

impl CopyNotice {
    fn from_text(text: &str) -> Option<Self> {
        (!text.is_empty()).then(|| Self {
            lines: text.split('\n').count(),
            bytes: text.len(),
        })
    }

    fn message(self) -> String {
        let unit = if self.lines == 1 { "line" } else { "lines" };
        format!(
            "Copied {} {} ({})",
            self.lines,
            unit,
            self.bytes.human_count_bytes()
        )
    }
}

struct CopyFeedbackState<'a> {
    notice: &'a mut State<Option<CopyNotice>>,
    deadline: &'a mut Ref<Option<Instant>>,
    wake: &'a Arc<Notify>,
}

impl CopyFeedbackState<'_> {
    fn show(&mut self, text: &str) {
        if let Some(notice) = CopyNotice::from_text(text) {
            self.notice.set(Some(notice));
            self.deadline
                .set(Some(Instant::now() + COPY_NOTICE_DURATION));
            self.wake.notify_one();
        }
    }

    fn clear(&mut self) {
        self.notice.set(None);
        self.deadline.set(None);
    }
}

/// Mutable selection state passed to event handlers.
struct SelectionState<'a> {
    has_selection: bool,
    anchor: Option<(usize, usize)>,
    cursor: Option<(usize, usize)>,
    logs: &'a Arc<VecDeque<String>>,
    buffer_start_line: usize,
    anchor_state: &'a mut State<Option<(usize, usize)>>,
    cursor_state: &'a mut State<Option<(usize, usize)>>,
    is_selecting: &'a mut State<bool>,
}

impl SelectionState<'_> {
    fn clear(&mut self) {
        self.anchor_state.set(None);
        self.cursor_state.set(None);
        self.is_selecting.set(false);
    }
}

fn back_clears_selection(key_event: &KeyEvent, has_selection: bool) -> bool {
    has_selection && key_event.code == KeyCode::Esc
}

/// Handle keyboard input - updates local scroll state, no model lock needed
#[allow(clippy::too_many_arguments)]
fn handle_key_event(
    key_event: KeyEvent,
    action: Option<Action>,
    key_consumed: bool,
    emergency_interrupt: bool,
    ui_state: &Arc<RwLock<UiState>>,
    shutdown: &Arc<Shutdown>,
    command_tx: Option<&ProcessCommandSender>,
    attached: bool,
    scroll_offset: &mut State<usize>,
    follow_tail: &mut State<bool>,
    scroll_anchor: &mut State<Option<ScrollAnchor>>,
    buffer_start_line: usize,
    visual_rows: &[VisualRow],
    total_visual_rows: usize,
    viewport_height: usize,
    log_search: &mut State<Option<LogSearchState>>,
    copy_feedback: &mut CopyFeedbackState<'_>,
    sel: &mut SelectionState<'_>,
) {
    if handle_interrupt_prompt_action(action, emergency_interrupt, ui_state, shutdown, command_tx) {
        return;
    }

    if copy_feedback.notice.get().is_some() {
        copy_feedback.clear();
    }

    let max_offset = total_visual_rows.saturating_sub(viewport_height);
    let current_scroll_offset = resolved_scroll_offset(
        scroll_offset.get(),
        follow_tail.get(),
        scroll_anchor.get(),
        buffer_start_line,
        visual_rows,
        total_visual_rows,
        viewport_height,
    );

    if handle_log_search_key(
        &key_event,
        action,
        key_consumed,
        log_search,
        sel.logs,
        buffer_start_line,
        visual_rows,
        current_scroll_offset,
        viewport_height,
        max_offset,
        scroll_offset,
        follow_tail,
        scroll_anchor,
    ) {
        return;
    }

    let scroll_action = match action {
        Some(Action::LineDown) => Some(LogScrollAction::Forward(1)),
        Some(Action::LineUp) => Some(LogScrollAction::Backward(1)),
        Some(Action::HalfPageDown) => {
            Some(LogScrollAction::Forward(viewport_height.div_ceil(2).max(1)))
        }
        Some(Action::HalfPageUp) => Some(LogScrollAction::Backward(
            viewport_height.div_ceil(2).max(1),
        )),
        Some(Action::PageDown) => Some(LogScrollAction::Forward(viewport_height)),
        Some(Action::PageUp) => Some(LogScrollAction::Backward(viewport_height)),
        Some(Action::Top) => Some(LogScrollAction::Top),
        Some(Action::Bottom) => Some(LogScrollAction::Bottom),
        _ => None,
    };
    if let Some(action) = scroll_action {
        apply_scroll_action(
            action,
            current_scroll_offset,
            max_offset,
            scroll_offset,
            follow_tail,
            scroll_anchor,
            buffer_start_line,
            visual_rows,
        );
        return;
    }

    match action {
        Some(Action::Back) => {
            if back_clears_selection(&key_event, sel.has_selection) {
                sel.clear();
            } else if let Ok(mut ui) = ui_state.write() {
                ui.view_mode = ViewMode::Main;
            }
        }

        Some(Action::Copy) => {
            let selection = match (sel.anchor, sel.cursor) {
                (Some(anchor), Some(cursor)) => Some(Selection::from_anchor_cursor(anchor, cursor)),
                _ => None,
            };
            let text = text_for_yank(sel.logs, sel.buffer_start_line, selection.as_ref());
            if !text.is_empty() {
                copy_to_clipboard(&text);
                copy_feedback.show(&text);
            }
            if sel.has_selection {
                sel.clear();
            }
        }

        _ if emergency_interrupt => {
            if sel.has_selection {
                if let (Some(anchor), Some(cursor)) = (sel.anchor, sel.cursor) {
                    let selection = Selection::from_anchor_cursor(anchor, cursor);
                    let text = extract_selected_text(sel.logs, sel.buffer_start_line, &selection);
                    if !text.is_empty() {
                        copy_to_clipboard(&text);
                        copy_feedback.show(&text);
                    }
                }
                sel.clear();
            } else if !request_interrupt_prompt(command_tx, ui_state, attached) {
                shutdown.handle_interrupt();
            }
        }

        Some(_) | None => {}
    }
}

/// Handle mouse input - updates local scroll state and selection
#[allow(clippy::too_many_arguments)]
fn handle_mouse_event(
    mouse_event: FullscreenMouseEvent,
    scroll_offset: &mut State<usize>,
    follow_tail: &mut State<bool>,
    scroll_anchor: &mut State<Option<ScrollAnchor>>,
    buffer_start_line: usize,
    total_visual_rows: usize,
    viewport_height: usize,
    statusline_position: StatuslinePosition,
    footer_visible: bool,
    visual_rows: &[VisualRow],
    selection_anchor: &mut State<Option<(usize, usize)>>,
    selection_cursor: &mut State<Option<(usize, usize)>>,
    is_selecting: &mut State<bool>,
) {
    let scroll_lines = 3; // Lines to scroll per wheel tick
    let max_offset = total_visual_rows.saturating_sub(viewport_height);
    let current_scroll_offset = resolved_scroll_offset(
        scroll_offset.get(),
        follow_tail.get(),
        scroll_anchor.get(),
        buffer_start_line,
        visual_rows,
        total_visual_rows,
        viewport_height,
    );

    // Map a terminal (row, col) to the logical (log_line_idx, visual_col)
    // selection coordinate. visual_col is a character offset into the *logical*
    // log line, accounting for the wrap segment that was clicked.
    let content_start_row = match (statusline_position, footer_visible) {
        (StatuslinePosition::Top, true) => 2,
        (StatuslinePosition::Bottom | StatuslinePosition::Inline, _) => 1,
        _ => 1,
    };
    let map_to_logical = |row: usize, col: usize| -> Option<(usize, usize)> {
        let visible_row_idx = row.checked_sub(content_start_row)?;
        let visual_row_idx = current_scroll_offset + visible_row_idx;
        let vrow = visual_rows.get(visual_row_idx)?;
        let col_in_segment = col.saturating_sub(LINE_NUM_PREFIX_WIDTH);
        Some((
            buffer_start_line + vrow.log_idx,
            vrow.char_start + col_in_segment,
        ))
    };

    match mouse_event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let row = mouse_event.row as usize;
            let col = mouse_event.column as usize;

            if row < content_start_row || row >= content_start_row + viewport_height {
                return;
            }

            let Some(pos) = map_to_logical(row, col) else {
                return;
            };

            selection_anchor.set(Some(pos));
            selection_cursor.set(Some(pos));
            is_selecting.set(true);
            pause_at_offset(
                scroll_offset,
                follow_tail,
                scroll_anchor,
                buffer_start_line,
                visual_rows,
                current_scroll_offset,
            );
        }

        MouseEventKind::Drag(MouseButton::Left) => {
            if !is_selecting.get() {
                return;
            }

            let row = mouse_event.row as usize;
            let col = mouse_event.column as usize;

            let clamped_row = row.clamp(
                content_start_row,
                content_start_row + viewport_height.saturating_sub(1),
            );
            let Some(pos) = map_to_logical(clamped_row, col) else {
                return;
            };

            selection_cursor.set(Some(pos));
        }

        MouseEventKind::Up(MouseButton::Left) => {
            is_selecting.set(false);

            // If anchor == cursor, it was just a click - clear selection
            if let (Some(anchor), Some(cursor)) = (selection_anchor.get(), selection_cursor.get())
                && anchor == cursor
            {
                selection_anchor.set(None);
                selection_cursor.set(None);
            }
        }

        MouseEventKind::ScrollDown => {
            let next = (current_scroll_offset + scroll_lines).min(max_offset);
            if next == max_offset {
                follow_at_bottom(scroll_offset, follow_tail, scroll_anchor, max_offset);
            } else {
                pause_at_offset(
                    scroll_offset,
                    follow_tail,
                    scroll_anchor,
                    buffer_start_line,
                    visual_rows,
                    next,
                );
            }
        }
        MouseEventKind::ScrollUp => {
            pause_at_offset(
                scroll_offset,
                follow_tail,
                scroll_anchor,
                buffer_start_line,
                visual_rows,
                current_scroll_offset.saturating_sub(scroll_lines),
            );
        }
        _ => {}
    }
}

/// Copy text to clipboard using OSC 52 escape sequence.
/// This works in most modern terminals including over SSH.
fn copy_to_clipboard(text: &str) {
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    // Write OSC 52 sequence to stderr (matches TUI output target)
    let _ = write!(std::io::stderr(), "\x1b]52;c;{}\x07", encoded);
    let _ = std::io::stderr().flush();
}

/// Extract selected text from log lines, stripping ANSI codes.
fn extract_selected_text(
    logs: &VecDeque<String>,
    buffer_start_line: usize,
    selection: &Selection,
) -> String {
    let mut lines = Vec::new();
    if logs.is_empty() {
        return String::new();
    }
    let Some(buffer_end_line) = buffer_start_line.checked_add(logs.len() - 1) else {
        return String::new();
    };
    let first_line = selection.start.0.max(buffer_start_line);
    let last_line = selection.end.0.min(buffer_end_line);
    if first_line > last_line {
        return String::new();
    }

    for absolute_line in first_line..=last_line {
        let Some(line) = logs.get(absolute_line - buffer_start_line) else {
            continue;
        };

        if let Some((start_col, end_col)) =
            selection.line_range(absolute_line, line.chars().count())
        {
            lines.push(char_slice(line, start_col, end_col));
        }
    }

    lines.join("\n")
}

fn text_for_yank(
    logs: &VecDeque<String>,
    buffer_start_line: usize,
    selection: Option<&Selection>,
) -> String {
    match selection {
        Some(selection) => extract_selected_text(logs, buffer_start_line, selection),
        None => logs.iter().cloned().collect::<Vec<_>>().join("\n"),
    }
}

fn search_result_text(search: LogSearchView<'_>) -> String {
    if search.query.is_empty() {
        return "type to search".to_string();
    }
    if search.matches.is_empty() {
        return "no matches".to_string();
    }
    let current = search.current.and_then(|current| {
        search
            .matches
            .iter()
            .position(|search_match| search_cursor(*search_match) == current)
    });
    match current {
        Some(index) => format!("{}/{}", index + 1, search.matches.len()),
        None => format!("{} matches", search.matches.len()),
    }
}

fn expanded_footer_piece(content: impl Into<String>, color: Color) -> AnyElement<'static> {
    element!(Text(content: content.into(), color: color)).into_any()
}

/// Render the expanded view UI
#[cfg(test)]
fn render_expanded_view(
    state: &ExpandedViewState,
    visual_rows: &[VisualRow],
    width: u16,
    height: u16,
    ui: ExpandedViewUi<'_>,
) -> AnyElement<'static> {
    render_expanded_view_custom(state, visual_rows, width, height, ui, None)
}

fn render_expanded_view_custom(
    state: &ExpandedViewState,
    visual_rows: &[VisualRow],
    width: u16,
    height: u16,
    ui: ExpandedViewUi<'_>,
    customization: Option<&ExpandedCustomization>,
) -> AnyElement<'static> {
    let (interrupt_prompt_active, interrupt_prompt_attached) = ui.interrupt_prompt;
    let statusline_position = customization
        .map(|customization| customization.preferences.statusline.position)
        .unwrap_or_default();
    let footer_visible = customization.is_none_or(|customization| {
        customization.preferences.statusline.enabled
            || interrupt_prompt_active
            || ui.search.is_some()
            || ui.copy_notice.is_some()
    });
    let viewport_height = calculate_viewport_height(height, footer_visible);
    let total_rows = visual_rows.len();

    let clamped_offset = resolved_scroll_offset(
        state.scroll_offset,
        ui.scroll_mode.follow_tail,
        ui.scroll_mode.anchor,
        state.buffer_start_line,
        visual_rows,
        total_rows,
        viewport_height,
    );

    let start = clamped_offset.min(total_rows);
    let end = (start + viewport_height).min(total_rows);
    let visible_rows: &[VisualRow] = &visual_rows[start..end];

    let line_elements = build_line_elements(
        visible_rows,
        &state.logs,
        state.buffer_start_line,
        ui.selection,
        ui.search,
    );
    let padding_elements = build_padding_elements(visible_rows.len(), viewport_height);

    let mut content_elements = line_elements;
    content_elements.extend(padding_elements);

    let progress = build_progress_indicator(clamped_offset, viewport_height, total_rows);
    let follow_status = if ui.scroll_mode.follow_tail {
        "FOLLOWING"
    } else {
        "PAUSED"
    };
    let compact_follow_status = if ui.scroll_mode.follow_tail { "F" } else { "P" };

    let configured_footer = customization
        .filter(|customization| !customization.preferences.uses_default_statusline())
        .map(|customization| {
            let mode = if interrupt_prompt_active || ui.copy_notice.is_some() {
                StatuslineMode::Prompt
            } else if ui.search.is_some() {
                StatuslineMode::Search
            } else {
                StatuslineMode::Logs
            };
            let (key_context, actions) = match mode {
                StatuslineMode::Prompt => (KeyContext::Prompt, Vec::new()),
                StatuslineMode::Search if ui.search.is_some_and(|search| search.editing) => (
                    KeyContext::LogSearch,
                    vec![
                        Action::NextMatch,
                        Action::PreviousMatch,
                        Action::Accept,
                        Action::Cancel,
                    ],
                ),
                StatuslineMode::Search => (
                    KeyContext::Logs,
                    vec![
                        Action::NextMatch,
                        Action::PreviousMatch,
                        Action::Search,
                        Action::Back,
                    ],
                ),
                StatuslineMode::Logs => (
                    KeyContext::Logs,
                    vec![
                        Action::LineDown,
                        Action::LineUp,
                        Action::HalfPageDown,
                        Action::HalfPageUp,
                        Action::PageDown,
                        Action::PageUp,
                        Action::Top,
                        Action::Bottom,
                        Action::Search,
                        Action::Copy,
                        Action::Back,
                    ],
                ),
                StatuslineMode::Main => unreachable!(),
            };
            let key_hints = if interrupt_prompt_active {
                Some(interrupt_prompt_key_hints(
                    &customization.keymap,
                    interrupt_prompt_attached,
                    width,
                ))
            } else {
                action_key_hints(&customization.keymap, key_context, actions, width)
            };
            let search_result = ui.search.map(search_result_text);
            let search_current = ui.search.and_then(|search| {
                search.current.and_then(|current| {
                    search
                        .matches
                        .iter()
                        .position(|item| search_cursor(*item) == current)
                        .map(|index| index + 1)
                })
            });
            let data = StatuslineData {
                context: (*customization.context).clone(),
                log_mode: Some(follow_status.to_lowercase()),
                log_current: Some(state.buffer_start_line + end),
                log_total: Some(state.buffer_start_line + state.logs.len()),
                retained_logs: Some(state.logs.len()),
                discarded_logs: Some(state.buffer_start_line),
                search_query: ui.search.map(|search| search.query.to_string()),
                search_current,
                search_total: ui.search.map(|search| search.matches.len()),
                search_result,
                prompt: if interrupt_prompt_active {
                    Some(if interrupt_prompt_attached {
                        "Detach or stop the process manager?".to_string()
                    } else {
                        "Quit devenv? Nothing has been stopped yet.".to_string()
                    })
                } else {
                    ui.copy_notice.map(CopyNotice::message)
                },
                pending_key: customization.pending_key.clone(),
                key_hints,
                ..StatuslineData::default()
            };
            crate::view::build_configured_statusline(
                render_statusline(
                    mode,
                    width.saturating_sub(2),
                    &customization.preferences,
                    &data,
                ),
                &customization.preferences.theme,
                width,
            )
        });

    let footer = if let Some(footer) = configured_footer {
        footer
    } else if interrupt_prompt_active {
        let compact = width < 120;
        let status = if width < 60 {
            format!("{compact_follow_status} ")
        } else {
            format!("{} \u{2502} {} \u{2502} ", follow_status, progress)
        };
        let mut action_children = Vec::new();
        if interrupt_prompt_attached && compact {
            action_children.push(expanded_footer_piece("Detach? ", Color::AnsiValue(245)));
            action_children.push(expanded_footer_piece("^C", COLOR_INTERACTIVE));
            action_children.push(expanded_footer_piece(":detach ", Color::AnsiValue(245)));
            action_children.push(expanded_footer_piece("s", COLOR_INTERACTIVE));
            action_children.push(expanded_footer_piece(":stop ", Color::AnsiValue(245)));
            action_children.push(expanded_footer_piece("Esc", COLOR_INTERACTIVE));
            action_children.push(expanded_footer_piece(":watch", Color::AnsiValue(245)));
        } else if interrupt_prompt_attached {
            action_children.push(expanded_footer_piece(
                "Detach or stop the process manager?  ",
                Color::AnsiValue(245),
            ));
            action_children.push(expanded_footer_piece("Ctrl-C", COLOR_INTERACTIVE));
            action_children.push(expanded_footer_piece(":detach  ", Color::AnsiValue(245)));
            action_children.push(expanded_footer_piece("s", COLOR_INTERACTIVE));
            action_children.push(expanded_footer_piece(
                ":stop manager  ",
                Color::AnsiValue(245),
            ));
            action_children.push(expanded_footer_piece("Esc", COLOR_INTERACTIVE));
            action_children.push(expanded_footer_piece(
                ":keep watching",
                Color::AnsiValue(245),
            ));
        } else if compact {
            action_children.push(expanded_footer_piece("Quit? ", Color::AnsiValue(245)));
            action_children.push(expanded_footer_piece("c", COLOR_INTERACTIVE));
            action_children.push(expanded_footer_piece(":run ", Color::AnsiValue(245)));
            action_children.push(expanded_footer_piece("q", COLOR_INTERACTIVE));
            action_children.push(expanded_footer_piece(":quit ", Color::AnsiValue(245)));
            action_children.push(expanded_footer_piece("^C", COLOR_INTERACTIVE));
            action_children.push(expanded_footer_piece(":quit", Color::AnsiValue(245)));
        } else {
            action_children.push(expanded_footer_piece(
                "Quit devenv? Nothing has been stopped yet  ",
                Color::AnsiValue(245),
            ));
            action_children.push(expanded_footer_piece("c", COLOR_INTERACTIVE));
            action_children.push(expanded_footer_piece(
                ":keep running  ",
                Color::AnsiValue(245),
            ));
            action_children.push(expanded_footer_piece("q", COLOR_INTERACTIVE));
            action_children.push(expanded_footer_piece(":quit  ", Color::AnsiValue(245)));
            action_children.push(expanded_footer_piece("Ctrl-C", COLOR_INTERACTIVE));
            action_children.push(expanded_footer_piece(":quit", Color::AnsiValue(245)));
        }
        element!(View(
            flex_direction: FlexDirection::Row,
            width: 100pct,
            overflow: Overflow::Hidden,
        ) {
            View(flex_shrink: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                Text(content: status, color: Color::AnsiValue(245))
            }
            View(flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                #(action_children)
            }
        })
        .into_any()
    } else if let Some(search) = ui.search.filter(|search| search.editing) {
        let compact = width < 60;
        let prompt = if compact {
            format!("/{}", search.query)
        } else {
            format!("Search logs: /{}", search.query)
        };
        let result = search_result_text(search);
        let prompt_color = if search.matches.is_empty() && !search.query.is_empty() {
            Color::AnsiValue(160)
        } else {
            Color::AnsiValue(220)
        };
        let mut action_children = vec![expanded_footer_piece(
            format!("{result}  "),
            Color::AnsiValue(245),
        )];
        action_children.push(expanded_footer_piece("Enter", COLOR_INTERACTIVE));
        if !compact {
            action_children.push(expanded_footer_piece(":select  ", Color::AnsiValue(245)));
        } else {
            action_children.push(expanded_footer_piece("  ", Color::AnsiValue(245)));
        }
        action_children.push(expanded_footer_piece("Esc", COLOR_INTERACTIVE));
        if !compact {
            action_children.push(expanded_footer_piece(":cancel", Color::AnsiValue(245)));
        }
        element!(View(
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            width: 100pct,
            overflow: Overflow::Hidden,
        ) {
            View(flex_grow: 1.0_f32, flex_shrink: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                Text(content: prompt, color: prompt_color, weight: Weight::Bold)
            }
            View(flex_direction: FlexDirection::Row, flex_shrink: 0.0, margin_left: 1) {
                #(action_children)
            }
        })
        .into_any()
    } else if let Some(notice) = ui.copy_notice {
        if width < 60 {
            element!(Text(
                content: notice.message(),
                color: COLOR_COMPLETED,
                weight: Weight::Bold
            ))
            .into_any()
        } else {
            element!(View(
                flex_direction: FlexDirection::Row,
                width: 100pct,
                overflow: Overflow::Hidden,
            ) {
                View(flex_shrink: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                    Text(
                        content: format!("{} \u{2502} {} \u{2502} ", follow_status, progress),
                        color: COLOR_COMPLETED,
                        weight: Weight::Bold
                    )
                }
                View(flex_shrink: 0.0) {
                    Text(
                        content: notice.message(),
                        color: COLOR_COMPLETED,
                        weight: Weight::Bold
                    )
                }
            })
            .into_any()
        }
    } else if let Some(search) = ui.search {
        let result = search_result_text(search);
        let compact = width < 90;
        let search_color = if search.matches.is_empty() {
            Color::AnsiValue(160)
        } else {
            Color::AnsiValue(220)
        };
        let mut action_children = vec![
            expanded_footer_piece(format!("  {result}  "), Color::AnsiValue(245)),
            expanded_footer_piece("n/N", COLOR_INTERACTIVE),
        ];
        if compact {
            action_children.push(expanded_footer_piece("  ", Color::AnsiValue(245)));
        } else {
            action_children.push(expanded_footer_piece(":match  ", Color::AnsiValue(245)));
        }
        action_children.push(expanded_footer_piece("/", COLOR_INTERACTIVE));
        action_children.push(expanded_footer_piece(
            if compact { "  " } else { ":new search  " },
            Color::AnsiValue(245),
        ));
        action_children.push(expanded_footer_piece("Esc", COLOR_INTERACTIVE));
        if compact {
            action_children.push(expanded_footer_piece("  ", Color::AnsiValue(245)));
        } else {
            action_children.push(expanded_footer_piece(":clear  ", Color::AnsiValue(245)));
        }
        action_children.push(expanded_footer_piece("q", COLOR_INTERACTIVE));
        if !compact {
            action_children.push(expanded_footer_piece(":back", Color::AnsiValue(245)));
        }
        element!(View(
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            width: 100pct,
            overflow: Overflow::Hidden,
        ) {
            View(flex_shrink: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                Text(
                    content: if width < 60 {
                        format!("{compact_follow_status} ")
                    } else {
                        format!("{} \u{2502} {}  ", follow_status, progress)
                    },
                    color: Color::AnsiValue(245)
                )
            }
            View(flex_grow: 1.0_f32, flex_shrink: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                Text(content: format!("/{}", search.query), color: search_color)
            }
            View(flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                #(action_children)
            }
        })
        .into_any()
    } else {
        let compact = width < 120;
        let status = if width < 60 {
            format!("{compact_follow_status} ")
        } else {
            format!("{} \u{2502} {} \u{2502} ", follow_status, progress)
        };
        let mut action_children = Vec::new();
        if compact {
            action_children.push(expanded_footer_piece(
                if ui.selection.is_some() {
                    if width < 60 { "y/^C " } else { "y/Ctrl-C " }
                } else {
                    "y "
                },
                COLOR_INTERACTIVE,
            ));
            for key in ["↑↓ j/k ", "^D/^U ", "^F/^B ", "/ ", "g/G ", "q"] {
                action_children.push(expanded_footer_piece(key, COLOR_INTERACTIVE));
            }
        } else {
            if ui.selection.is_some() {
                action_children.push(expanded_footer_piece("y", COLOR_INTERACTIVE));
                action_children.push(expanded_footer_piece(":copy  ", Color::AnsiValue(245)));
                action_children.push(expanded_footer_piece("Ctrl-C", COLOR_INTERACTIVE));
                action_children.push(expanded_footer_piece(":copy  ", Color::AnsiValue(245)));
            } else {
                action_children.push(expanded_footer_piece("y", COLOR_INTERACTIVE));
                action_children.push(expanded_footer_piece(":copy all  ", Color::AnsiValue(245)));
            }
            for (key, label) in [
                ("↑↓ j/k", ":line  "),
                ("^D/^U", ":half  "),
                ("^F/^B", ":page  "),
                ("/", ":search  "),
                ("g/G", ":top/follow  "),
                ("q", ":back"),
            ] {
                action_children.push(expanded_footer_piece(key, COLOR_INTERACTIVE));
                action_children.push(expanded_footer_piece(label, Color::AnsiValue(245)));
            }
        }
        element!(View(
            flex_direction: FlexDirection::Row,
            width: 100pct,
            overflow: Overflow::Hidden,
        ) {
            View(flex_shrink: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                Text(content: status, color: Color::AnsiValue(245))
            }
            View(flex_direction: FlexDirection::Row, flex_shrink: 0.0) {
                #(action_children)
            }
        })
        .into_any()
    };

    let header = element!(View(height: 1, padding_left: 1, padding_right: 1) {
        Text(
            content: format!("\u{2500}\u{2500}\u{2500} {} \u{2500}\u{2500}\u{2500}", state.activity_name),
            color: Color::Cyan,
            weight: Weight::Bold
        )
    })
    .into_any();
    let content = element!(View(flex_grow: 1.0_f32, flex_direction: FlexDirection::Column) {
        #(content_elements)
    })
    .into_any();
    let statusline = footer_visible.then(|| {
        element!(View(height: 1, padding_left: 1, padding_right: 1) {
            #(footer)
        })
        .into_any()
    });
    let children = match (statusline_position, statusline) {
        (StatuslinePosition::Top, Some(statusline)) => vec![statusline, header, content],
        (StatuslinePosition::Bottom | StatuslinePosition::Inline, Some(statusline)) => {
            vec![header, content, statusline]
        }
        (_, None) => vec![header, content],
    };

    element!(View(
        flex_direction: FlexDirection::Column,
        height: height as u32,
        width: width as u32
    ) {
        #(children)
    })
    .into_any()
}

/// Build elements for visible log lines.
///
/// Continuation rows of a wrapped log line share the same log line number;
/// the gutter shows the line number once on the first row and blanks on
/// continuation rows so the underlying log line is unambiguous.
fn build_line_elements(
    visible_rows: &[VisualRow],
    logs: &VecDeque<String>,
    buffer_start_line: usize,
    selection: Option<&Selection>,
    search: Option<LogSearchView<'_>>,
) -> Vec<AnyElement<'static>> {
    let mut elements = Vec::with_capacity(visible_rows.len());

    for vrow in visible_rows {
        let Some(line) = logs.get(vrow.log_idx) else {
            continue;
        };

        let display_segment = char_slice(line, vrow.char_start, vrow.char_end);

        let absolute_line = buffer_start_line + vrow.log_idx;
        let line_number = if vrow.char_start == 0 {
            (vrow.log_idx + 1).to_string()
        } else {
            String::new()
        };
        let line_prefix = format!(
            "{:>width$}{}",
            line_number,
            LINE_NUM_SEPARATOR,
            width = LINE_NUM_DIGITS
        );

        // Intersect the per-log-line selection with the [char_start, char_end)
        // window this row represents, in logical char coordinates.
        let sel_range = selection.and_then(|s| {
            let (start_col, end_col) = s.line_range(absolute_line, line.chars().count())?;
            let s = start_col.max(vrow.char_start);
            let e = end_col.min(vrow.char_end);
            (e > s).then(|| (s - vrow.char_start, e - vrow.char_start))
        });
        let search_ranges: Vec<_> = search
            .into_iter()
            .flat_map(|search| {
                search.matches.iter().filter_map(move |search_match| {
                    if search_match.log_line != absolute_line {
                        return None;
                    }
                    let start = search_match.char_start.max(vrow.char_start);
                    let end = search_match.char_end.min(vrow.char_end);
                    (end > start).then(|| {
                        (
                            start - vrow.char_start,
                            end - vrow.char_start,
                            search.current == Some(search_cursor(*search_match)),
                        )
                    })
                })
            })
            .collect();

        if let Some((start_col, end_col)) = sel_range {
            let before = char_slice(&display_segment, 0, start_col);
            let selected = char_slice(&display_segment, start_col, end_col);
            let after: String = display_segment.chars().skip(end_col).collect();

            elements.push(
                element! {
                    View(height: 1, flex_direction: FlexDirection::Row) {
                        Text(
                            content: line_prefix,
                            color: Color::AnsiValue(250)
                        )
                        Text(
                            content: before,
                            color: Color::AnsiValue(250)
                        )
                        View(background_color: Color::AnsiValue(250)) {
                            Text(
                                content: selected,
                                color: Color::AnsiValue(232)
                            )
                        }
                        Text(
                            content: after,
                            color: Color::AnsiValue(250)
                        )
                    }
                }
                .into_any(),
            );
        } else if !search_ranges.is_empty() {
            let mut cursor = 0;
            let mut segments = Vec::new();
            for (start, end, current) in search_ranges {
                if start > cursor {
                    segments.push(
                        element!(Text(
                            content: char_slice(&display_segment, cursor, start),
                            color: Color::AnsiValue(250)
                        ))
                        .into_any(),
                    );
                }
                let background = if current {
                    Color::AnsiValue(220)
                } else {
                    Color::AnsiValue(58)
                };
                let foreground = if current {
                    Color::AnsiValue(232)
                } else {
                    Color::AnsiValue(230)
                };
                segments.push(
                    element!(View(background_color: background) {
                        Text(
                            content: char_slice(&display_segment, start, end),
                            color: foreground
                        )
                    })
                    .into_any(),
                );
                cursor = end;
            }
            if cursor < display_segment.chars().count() {
                segments.push(
                    element!(Text(
                        content: char_slice(&display_segment, cursor, display_segment.chars().count()),
                        color: Color::AnsiValue(250)
                    ))
                    .into_any(),
                );
            }
            elements.push(
                element! {
                    View(height: 1, flex_direction: FlexDirection::Row) {
                        Text(
                            content: line_prefix,
                            color: Color::AnsiValue(250)
                        )
                        #(segments)
                    }
                }
                .into_any(),
            );
        } else {
            elements.push(
                element! {
                    View(height: 1) {
                        Text(
                            content: format!("{}{}", line_prefix, display_segment),
                            color: Color::AnsiValue(250)
                        )
                    }
                }
                .into_any(),
            );
        }
    }

    elements
}

/// Build empty padding elements to fill the viewport
fn build_padding_elements(filled_lines: usize, viewport_height: usize) -> Vec<AnyElement<'static>> {
    let padding_count = viewport_height.saturating_sub(filled_lines);
    let mut elements = Vec::with_capacity(padding_count);

    for _ in 0..padding_count {
        elements.push(
            element! {
                View(height: 1) {
                    Text(content: "~".to_string(), color: Color::AnsiValue(238))
                }
            }
            .into_any(),
        );
    }

    elements
}

/// Build the progress indicator string
fn build_progress_indicator(offset: usize, viewport_height: usize, total_lines: usize) -> String {
    if total_lines == 0 {
        "Empty".to_string()
    } else {
        let start = offset + 1;
        let end = (offset + viewport_height).min(total_lines);
        format!("{}-{}/{}", start, end, total_lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    fn expanded_ui<'a>(
        selection: Option<&'a Selection>,
        search: Option<LogSearchView<'a>>,
        copy_notice: Option<CopyNotice>,
        follow_tail: bool,
        interrupt_prompt: (bool, bool),
    ) -> ExpandedViewUi<'a> {
        ExpandedViewUi {
            selection,
            search,
            copy_notice,
            scroll_mode: ScrollMode {
                follow_tail,
                anchor: None,
            },
            interrupt_prompt,
        }
    }

    #[test]
    fn test_follow_mode_resolves_to_latest_rows() {
        assert_eq!(resolved_scroll_offset(0, true, None, 0, &[], 100, 20), 80);
        assert_eq!(resolved_scroll_offset(12, false, None, 0, &[], 100, 20), 12);
        assert_eq!(resolved_scroll_offset(80, true, None, 0, &[], 140, 20), 120);
        assert_eq!(resolved_scroll_offset(120, false, None, 0, &[], 60, 20), 40);
    }

    #[test]
    fn test_keyboard_scroll_shortcuts_use_vim_distances() {
        let action = |code, control| {
            let mut event = KeyEvent::new(KeyEventKind::Press, code);
            if control {
                event.modifiers = KeyModifiers::CONTROL;
            }
            keyboard_scroll_action(&event, 21)
        };

        assert_eq!(
            action(KeyCode::Char('d'), true),
            Some(LogScrollAction::Forward(11))
        );
        assert_eq!(
            action(KeyCode::Char('u'), true),
            Some(LogScrollAction::Backward(11))
        );
        assert_eq!(
            action(KeyCode::Char('f'), true),
            Some(LogScrollAction::Forward(21))
        );
        assert_eq!(
            action(KeyCode::Char('b'), true),
            Some(LogScrollAction::Backward(21))
        );
        assert_eq!(
            action(KeyCode::PageDown, false),
            Some(LogScrollAction::Forward(21))
        );
        assert_eq!(
            action(KeyCode::PageUp, false),
            Some(LogScrollAction::Backward(21))
        );
        assert_eq!(action(KeyCode::Char('d'), false), None);
    }

    #[test]
    fn only_escape_clears_a_selection_before_leaving_logs() {
        let escape = KeyEvent::new(KeyEventKind::Press, KeyCode::Esc);
        let quit = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('q'));
        let mut ctrl_e = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('e'));
        ctrl_e.modifiers = KeyModifiers::CONTROL;

        assert!(back_clears_selection(&escape, true));
        assert!(!back_clears_selection(&quit, true));
        assert!(!back_clears_selection(&ctrl_e, true));
        assert!(!back_clears_selection(&escape, false));
    }

    #[test]
    fn test_log_search_matches_case_insensitively() {
        assert_eq!(
            case_insensitive_match_ranges("Error error ERROR", "eRrOr"),
            vec![(0, 5), (6, 11), (12, 17)]
        );
        assert_eq!(
            case_insensitive_match_ranges("Ärger ärger", "äR"),
            vec![(0, 2), (6, 8)]
        );

        let logs = VecDeque::from(["ready".to_string(), "ERROR again".to_string()]);
        assert_eq!(
            find_log_matches(&logs, 20, "error"),
            vec![LogSearchMatch {
                log_line: 21,
                char_start: 0,
                char_end: 5,
            }]
        );
    }

    #[test]
    fn test_log_search_navigation_wraps() {
        let matches = [
            LogSearchMatch {
                log_line: 4,
                char_start: 2,
                char_end: 5,
            },
            LogSearchMatch {
                log_line: 8,
                char_start: 6,
                char_end: 9,
            },
        ];

        assert_eq!(
            adjacent_search_match(&matches, Some(search_cursor(matches[0])), true),
            Some(matches[1])
        );
        assert_eq!(
            adjacent_search_match(&matches, Some(search_cursor(matches[1])), true),
            Some(matches[0])
        );
        assert_eq!(
            adjacent_search_match(&matches, Some(search_cursor(matches[0])), false),
            Some(matches[1])
        );
        assert_eq!(
            adjacent_search_match(&matches, None, true),
            Some(matches[0])
        );
        assert_eq!(
            adjacent_search_match(&matches, None, false),
            Some(matches[1])
        );
    }

    #[test]
    fn test_paused_anchor_survives_log_buffer_rotation() {
        let logs = VecDeque::from([
            "one".to_string(),
            "two".to_string(),
            "three".to_string(),
            "four".to_string(),
            "five".to_string(),
        ]);
        let rows = build_visual_rows(&logs, 20);
        let anchor = Some(ScrollAnchor {
            log_line: 102,
            char_start: 0,
        });

        assert_eq!(
            resolved_scroll_offset(2, false, anchor, 100, &rows, 5, 1),
            2
        );
        assert_eq!(
            resolved_scroll_offset(2, false, anchor, 101, &rows, 5, 1),
            1
        );
        assert_eq!(
            resolved_scroll_offset(2, false, anchor, 103, &rows, 5, 1),
            0
        );
    }

    #[test]
    fn test_render_expanded_view_shows_follow_state() {
        let state = ExpandedViewState {
            activity_name: "api".to_string(),
            scroll_offset: 0,
            logs: Arc::new(VecDeque::from(["ready".to_string()])),
            buffer_start_line: 0,
        };
        let visual_rows = build_visual_rows(&state.logs, content_width_for(120));

        let mut following = render_expanded_view(
            &state,
            &visual_rows,
            120,
            8,
            ExpandedViewUi {
                selection: None,
                search: None,
                copy_notice: None,
                scroll_mode: ScrollMode {
                    follow_tail: true,
                    anchor: None,
                },
                interrupt_prompt: (false, false),
            },
        );
        let following_output = following.render(Some(120)).to_string();
        assert!(following_output.contains("FOLLOWING"));
        assert!(following_output.contains("y:copy all"));
        assert!(following_output.contains("↑↓ j/k:line"));
        assert!(following_output.contains("^D/^U:half"));

        let mut paused = render_expanded_view(
            &state,
            &visual_rows,
            120,
            8,
            ExpandedViewUi {
                selection: None,
                search: None,
                copy_notice: None,
                scroll_mode: ScrollMode {
                    follow_tail: false,
                    anchor: None,
                },
                interrupt_prompt: (false, false),
            },
        );
        let paused_output = paused.render(Some(120)).to_string();
        assert!(paused_output.contains("PAUSED"));
        assert!(paused_output.contains("g/G:top/follow"));
    }

    #[test]
    fn expanded_footer_preserves_actions_across_widths() {
        let state = ExpandedViewState {
            activity_name: "api".to_string(),
            scroll_offset: 0,
            logs: Arc::new((0..1234).map(|line| format!("line {line}")).collect()),
            buffer_start_line: 0,
        };
        let matches = find_log_matches(&state.logs, state.buffer_start_line, "line");
        let selection = Selection::from_anchor_cursor((0, 0), (0, 1));

        for width in 40u16..=240 {
            let visual_rows = build_visual_rows(&state.logs, content_width_for(width));
            let compact_controls = "↑↓ j/k ^D/^U ^F/^B / g/G q";
            let normal_controls = if width < 120 {
                compact_controls
            } else {
                "↑↓ j/k:line  ^D/^U:half  ^F/^B:page  /:search  g/G:top/follow  q:back"
            };
            let selection_copy = if width < 60 {
                "y/^C"
            } else if width < 120 {
                "y/Ctrl-C"
            } else {
                "y:copy  Ctrl-C:copy"
            };
            let search_edit_actions = if width < 60 {
                "Enter  Esc"
            } else {
                "Enter:select  Esc:cancel"
            };
            let search_actions = if width < 90 {
                "n/N  /  Esc  q"
            } else {
                "n/N:match  /:new search  Esc:clear  q:back"
            };
            let quit_actions = if width < 120 {
                "c:run q:quit ^C:quit"
            } else {
                "c:keep running  q:quit  Ctrl-C:quit"
            };
            let detach_actions = if width < 120 {
                "^C:detach s:stop Esc:watch"
            } else {
                "Ctrl-C:detach  s:stop manager  Esc:keep watching"
            };

            for (name, ui, expected) in [
                (
                    "normal",
                    expanded_ui(None, None, None, true, (false, false)),
                    normal_controls,
                ),
                (
                    "selection",
                    expanded_ui(Some(&selection), None, None, false, (false, false)),
                    selection_copy,
                ),
                (
                    "search-editing",
                    expanded_ui(
                        None,
                        Some(LogSearchView {
                            query: "a-very-long-search-query",
                            matches: &matches,
                            current: Some(search_cursor(matches[0])),
                            editing: true,
                        }),
                        None,
                        false,
                        (false, false),
                    ),
                    search_edit_actions,
                ),
                (
                    "search",
                    expanded_ui(
                        None,
                        Some(LogSearchView {
                            query: "a-very-long-search-query",
                            matches: &matches,
                            current: Some(search_cursor(matches[0])),
                            editing: false,
                        }),
                        None,
                        false,
                        (false, false),
                    ),
                    search_actions,
                ),
                (
                    "copy",
                    expanded_ui(
                        None,
                        None,
                        CopyNotice::from_text("ready"),
                        true,
                        (false, false),
                    ),
                    "Copied 1 line (5B)",
                ),
                (
                    "quit",
                    expanded_ui(None, None, None, true, (true, false)),
                    quit_actions,
                ),
                (
                    "detach",
                    expanded_ui(None, None, None, true, (true, true)),
                    detach_actions,
                ),
            ] {
                let mut element = render_expanded_view(&state, &visual_rows, width, 8, ui);
                let output = element.render(Some(width as usize)).to_string();
                let widest = output
                    .lines()
                    .map(UnicodeWidthStr::width)
                    .max()
                    .unwrap_or(0);
                assert!(
                    widest <= width as usize,
                    "{name} width {widest} exceeds terminal width {width}:\n{output}"
                );
                let footer = output.lines().last().unwrap_or_default();
                assert!(
                    footer.contains(expected),
                    "{name} footer lost actions at width {width}:\n{output}"
                );
                if name == "selection" {
                    assert!(
                        footer.contains(normal_controls),
                        "selected footer lost navigation at width {width}:\n{output}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_render_expanded_view_log_search() {
        let state = ExpandedViewState {
            activity_name: "api".to_string(),
            scroll_offset: 0,
            logs: Arc::new(VecDeque::from(["Error here".to_string()])),
            buffer_start_line: 10,
        };
        let visual_rows = build_visual_rows(&state.logs, content_width_for(120));
        let matches = find_log_matches(&state.logs, state.buffer_start_line, "error");

        let mut editing = render_expanded_view(
            &state,
            &visual_rows,
            120,
            8,
            ExpandedViewUi {
                selection: None,
                search: Some(LogSearchView {
                    query: "error",
                    matches: &matches,
                    current: Some(search_cursor(matches[0])),
                    editing: true,
                }),
                copy_notice: None,
                scroll_mode: ScrollMode {
                    follow_tail: false,
                    anchor: None,
                },
                interrupt_prompt: (false, false),
            },
        );
        let editing_output = editing.render(Some(100)).to_string();
        assert!(editing_output.contains("Search logs: /error"));
        assert!(editing_output.contains("1/1"));

        let mut selected = render_expanded_view(
            &state,
            &visual_rows,
            100,
            8,
            ExpandedViewUi {
                selection: None,
                search: Some(LogSearchView {
                    query: "error",
                    matches: &matches,
                    current: Some(search_cursor(matches[0])),
                    editing: false,
                }),
                copy_notice: None,
                scroll_mode: ScrollMode {
                    follow_tail: false,
                    anchor: None,
                },
                interrupt_prompt: (false, false),
            },
        );
        let selected_output = selected.render(Some(100)).to_string();
        assert!(selected_output.contains("/error"));
        assert!(selected_output.contains("n/N:match"));

        let mut missing = render_expanded_view(
            &state,
            &visual_rows,
            100,
            8,
            ExpandedViewUi {
                selection: None,
                search: Some(LogSearchView {
                    query: "missing",
                    matches: &[],
                    current: None,
                    editing: true,
                }),
                copy_notice: None,
                scroll_mode: ScrollMode {
                    follow_tail: false,
                    anchor: None,
                },
                interrupt_prompt: (false, false),
            },
        );
        let missing_output = missing.render(Some(100)).to_string();
        assert!(missing_output.contains("Search logs: /missing"));
        assert!(missing_output.contains("no matches"));
    }

    #[test]
    fn test_yank_uses_selection_or_complete_log_buffer() {
        let logs = VecDeque::from(["first".to_string(), "second".to_string()]);
        assert_eq!(text_for_yank(&logs, 10, None), "first\nsecond");

        let selection = Selection::from_anchor_cursor((10, 1), (11, 3));
        assert_eq!(text_for_yank(&logs, 10, Some(&selection)), "irst\nsec");
    }

    #[test]
    fn test_copy_notice_counts_and_renders_copied_text() {
        assert_eq!(CopyNotice::from_text(""), None);
        assert_eq!(
            CopyNotice::from_text("first\nsecond"),
            Some(CopyNotice {
                lines: 2,
                bytes: 12,
            })
        );
        assert!(
            CopyNotice::from_text("one")
                .unwrap()
                .message()
                .contains("1 line")
        );

        let state = ExpandedViewState {
            activity_name: "api".to_string(),
            scroll_offset: 0,
            logs: Arc::new(VecDeque::from(["first".to_string(), "second".to_string()])),
            buffer_start_line: 0,
        };
        let visual_rows = build_visual_rows(&state.logs, content_width_for(100));
        let mut element = render_expanded_view(
            &state,
            &visual_rows,
            100,
            8,
            ExpandedViewUi {
                selection: None,
                search: None,
                copy_notice: CopyNotice::from_text("first\nsecond"),
                scroll_mode: ScrollMode {
                    follow_tail: true,
                    anchor: None,
                },
                interrupt_prompt: (false, false),
            },
        );
        let output = element.render(Some(100)).to_string();
        assert!(output.contains("Copied 2 lines"));
        assert!(output.contains("12B"));
    }

    #[test]
    fn test_render_expanded_view_interrupt_prompt_footer() {
        let state = ExpandedViewState {
            activity_name: "api".to_string(),
            scroll_offset: 0,
            logs: Arc::new(VecDeque::new()),
            buffer_start_line: 0,
        };
        let visual_rows = build_visual_rows(&state.logs, content_width_for(120));

        let mut element = render_expanded_view(
            &state,
            &visual_rows,
            120,
            8,
            ExpandedViewUi {
                selection: None,
                search: None,
                copy_notice: None,
                scroll_mode: ScrollMode {
                    follow_tail: true,
                    anchor: None,
                },
                interrupt_prompt: (true, false),
            },
        );
        let output = element.render(Some(120)).to_string();

        assert!(output.contains("Quit devenv? Nothing has been stopped yet"));
        assert!(output.contains("c:keep running"));
        assert!(output.contains("q:quit"));
    }

    #[test]
    fn configured_interrupt_prompt_footer_stays_visible_and_contextual() {
        let state = ExpandedViewState {
            activity_name: "api".to_string(),
            scroll_offset: 0,
            logs: Arc::new(VecDeque::new()),
            buffer_start_line: 0,
        };
        let visual_rows = build_visual_rows(&state.logs, content_width_for(160));
        let mut preferences = crate::config::TuiPreferences::default();
        preferences.statusline.enabled = false;
        let customization = ExpandedCustomization {
            preferences: Arc::new(preferences),
            keymap: Arc::new(
                crate::config::KeybindingsConfig::default()
                    .resolve()
                    .unwrap(),
            ),
            context: Arc::new(crate::config::TuiRunContext::default()),
            pending_key: None,
        };

        let render = |attached| {
            let mut element = render_expanded_view_custom(
                &state,
                &visual_rows,
                160,
                8,
                expanded_ui(None, None, None, true, (true, attached)),
                Some(&customization),
            );
            element.render(Some(160)).to_string()
        };

        let starting = render(false);
        assert!(starting.contains("Quit devenv?"));
        assert!(starting.contains("q quit"));
        assert!(starting.contains("Ctrl-C quit"));
        assert!(!starting.contains("s stop manager"));

        let attached = render(true);
        assert!(attached.contains("Detach or stop"));
        assert!(attached.contains("s stop manager"));
        assert!(attached.contains("Ctrl-C detach"));
        assert!(!attached.contains("q quit"));
    }

    #[test]
    fn statusline_position_applies_to_expanded_logs() {
        let state = ExpandedViewState {
            activity_name: "api".to_string(),
            scroll_offset: 0,
            logs: Arc::new(VecDeque::from(["ready".to_string()])),
            buffer_start_line: 0,
        };
        let visual_rows = build_visual_rows(&state.logs, content_width_for(120));

        let render = |position| {
            let mut preferences = crate::config::TuiPreferences::default();
            preferences.statusline.position = position;
            let customization = ExpandedCustomization {
                preferences: Arc::new(preferences),
                keymap: Arc::new(
                    crate::config::KeybindingsConfig::default()
                        .resolve()
                        .unwrap(),
                ),
                context: Arc::new(crate::config::TuiRunContext::default()),
                pending_key: None,
            };
            let mut element = render_expanded_view_custom(
                &state,
                &visual_rows,
                120,
                8,
                expanded_ui(None, None, None, true, (false, false)),
                Some(&customization),
            );
            element.render(Some(120)).to_string()
        };

        for (position, expected_row) in [
            (StatuslinePosition::Top, 0),
            (StatuslinePosition::Bottom, 7),
            (StatuslinePosition::Inline, 7),
        ] {
            let output = render(position);
            assert_eq!(
                output
                    .lines()
                    .position(|line| line.contains("FOLLOWING"))
                    .unwrap(),
                expected_row
            );
        }
    }

    #[test]
    fn disabled_log_statusline_reclaims_the_footer_row() {
        assert_eq!(calculate_viewport_height(8, true), 6);
        assert_eq!(calculate_viewport_height(8, false), 7);
    }

    #[test]
    fn configured_log_footer_keeps_mode_specific_actions() {
        let state = ExpandedViewState {
            activity_name: "api".to_string(),
            scroll_offset: 0,
            logs: Arc::new(VecDeque::from(["ready".to_string()])),
            buffer_start_line: 0,
        };
        let visual_rows = build_visual_rows(&state.logs, content_width_for(400));
        let matches = find_log_matches(&state.logs, 0, "ready");
        let customization = ExpandedCustomization {
            preferences: Arc::new(crate::config::TuiPreferences {
                theme: crate::config::ThemeConfig {
                    preset: crate::config::ThemePreset::Terminal,
                    ..crate::config::ThemeConfig::default()
                },
                ..crate::config::TuiPreferences::default()
            }),
            keymap: Arc::new(
                crate::config::KeybindingsConfig::default()
                    .resolve()
                    .unwrap(),
            ),
            context: Arc::new(crate::config::TuiRunContext::default()),
            pending_key: None,
        };

        let mut logs = render_expanded_view_custom(
            &state,
            &visual_rows,
            400,
            8,
            expanded_ui(None, None, None, true, (false, false)),
            Some(&customization),
        );
        let logs = logs.render(Some(400)).to_string();
        assert!(logs.contains("Home top"));
        assert!(logs.contains("End bottom"));

        let mut compact_logs = render_expanded_view_custom(
            &state,
            &visual_rows,
            80,
            8,
            expanded_ui(None, None, None, true, (false, false)),
            Some(&customization),
        );
        let compact_logs = compact_logs.render(Some(80)).to_string();
        for key in ["^D", "^U", "^F", "^B", "g", "End", "/", "y", "q"] {
            assert!(
                compact_logs.contains(key),
                "missing {key:?}: {compact_logs:?}"
            );
        }

        let search = |editing| LogSearchView {
            query: "ready",
            matches: &matches,
            current: Some(search_cursor(matches[0])),
            editing,
        };
        let mut editing = render_expanded_view_custom(
            &state,
            &visual_rows,
            400,
            8,
            expanded_ui(None, Some(search(true)), None, false, (false, false)),
            Some(&customization),
        );
        let editing = editing.render(Some(400)).to_string();
        assert!(editing.contains("Enter accept"));
        assert!(editing.contains("Esc cancel"));

        let mut selected = render_expanded_view_custom(
            &state,
            &visual_rows,
            400,
            8,
            expanded_ui(None, Some(search(false)), None, false, (false, false)),
            Some(&customization),
        );
        let selected = selected.render(Some(400)).to_string();
        assert!(selected.contains("n next"));
        assert!(selected.contains("Shift+N previous"));
        assert!(selected.contains("/ search"));
        assert!(selected.contains("q back"));

        let mut copied = render_expanded_view_custom(
            &state,
            &visual_rows,
            400,
            8,
            expanded_ui(
                None,
                None,
                CopyNotice::from_text("ready"),
                true,
                (false, false),
            ),
            Some(&customization),
        );
        let copied = copied.render(Some(400)).to_string();
        assert!(copied.contains("Copied 1 line"));
        assert!(copied.contains("5B"));
    }

    #[test]
    fn test_build_visual_rows_short_line_fits_one_row() {
        let mut logs = VecDeque::new();
        logs.push_back("short".to_string());

        let rows = build_visual_rows(&logs, 10);

        assert_eq!(
            rows,
            vec![VisualRow {
                log_idx: 0,
                char_start: 0,
                char_end: 5,
            }]
        );
    }

    #[test]
    fn test_build_visual_rows_long_line_wraps() {
        let mut logs = VecDeque::new();
        logs.push_back("abcdefghij".to_string()); // exactly width=10
        logs.push_back("ABCDEFGHIJKLMNO".to_string()); // 15 chars, wraps to 2 rows

        let rows = build_visual_rows(&logs, 10);

        assert_eq!(
            rows,
            vec![
                VisualRow {
                    log_idx: 0,
                    char_start: 0,
                    char_end: 10
                },
                VisualRow {
                    log_idx: 1,
                    char_start: 0,
                    char_end: 10
                },
                VisualRow {
                    log_idx: 1,
                    char_start: 10,
                    char_end: 15
                },
            ]
        );
    }

    /// The full content of a long line appears in the rendered output spread
    /// across multiple visual rows, with no ellipsis truncation marker.
    #[test]
    fn test_render_shows_full_long_line_without_ellipsis() {
        let long: String = (0..80).map(|i| char::from(b'a' + (i as u8 % 26))).collect();
        let mut logs = VecDeque::new();
        logs.push_back(long.clone());
        let logs = Arc::new(logs);

        let state = ExpandedViewState {
            activity_name: "test".to_string(),
            scroll_offset: 0,
            logs: logs.clone(),
            buffer_start_line: 0,
        };
        let width: u16 = 30;
        let visual_rows = build_visual_rows(&state.logs, content_width_for(width));
        assert!(visual_rows.len() > 1);

        let mut element = render_expanded_view(
            &state,
            &visual_rows,
            width,
            (visual_rows.len() as u16) + 2,
            ExpandedViewUi {
                selection: None,
                search: None,
                copy_notice: None,
                scroll_mode: ScrollMode {
                    follow_tail: false,
                    anchor: None,
                },
                interrupt_prompt: (false, false),
            },
        );
        let output = element.render(Some(width as usize)).to_string();

        for vrow in &visual_rows {
            let segment = char_slice(&long, vrow.char_start, vrow.char_end);
            assert!(
                output.contains(&segment),
                "expected wrapped segment {:?} to appear in output:\n{}",
                segment,
                output
            );
        }
        assert!(!output.contains('…'));
    }

    /// Selecting across a wrapped log line should map the (log_idx, visual_col)
    /// pair back to the right segment on every visual row of that line.
    #[test]
    fn test_selection_spans_wrapped_segments() {
        let mut logs = VecDeque::new();
        logs.push_back("abcdefghijKLMNO".to_string());
        let logs_for_extract = logs.clone();
        let logs_arc = Arc::new(logs);

        let state = ExpandedViewState {
            activity_name: "test".to_string(),
            scroll_offset: 0,
            logs: logs_arc.clone(),
            buffer_start_line: 0,
        };
        let width: u16 = 18; // content_width = 10
        let visual_rows = build_visual_rows(&state.logs, content_width_for(width));
        assert_eq!(visual_rows.len(), 2);

        // Select columns [2..13] of the logical line, which spans the boundary
        // of the two wrap segments.
        let selection = Selection::from_anchor_cursor((0, 2), (0, 13));
        let mut element = render_expanded_view(
            &state,
            &visual_rows,
            width,
            6,
            ExpandedViewUi {
                selection: Some(&selection),
                search: None,
                copy_notice: None,
                scroll_mode: ScrollMode {
                    follow_tail: false,
                    anchor: None,
                },
                interrupt_prompt: (false, false),
            },
        );
        let output = element.render(Some(width as usize)).to_string();
        assert!(output.contains("cdefghij"));
        assert!(output.contains("KLM"));

        let extracted = extract_selected_text(&logs_for_extract, 0, &selection);
        assert_eq!(extracted, "cdefghijKLM");
    }
}
