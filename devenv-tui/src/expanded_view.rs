//! Expanded log view component.
//!
//! This component displays build logs in a fullscreen view using the alternate screen buffer.
//! It provides scrollable access to all log lines for a selected activity.
//! Scroll offset is managed as component-local state for immediate responsiveness.
//! Supports mouse-based text selection with OSC 52 clipboard copy.

use crate::TuiConfig;
use crate::app::{
    ExitFlag, ProcessCommandSender, handle_interrupt_prompt_key, request_interrupt_prompt,
};
use crate::model::{ActivityModel, UiState, ViewMode};
use base64::Engine;
use crossterm::event::MouseButton;
use iocraft::prelude::*;
use iocraft::{FullscreenMouseEvent, MouseEventKind};
use std::collections::VecDeque;
use std::io::Write as _;
use std::sync::{Arc, RwLock};
use tokio::sync::Notify;
use tokio_shutdown::Shutdown;

/// Width of the line-number field, e.g. "NNNNN".
const LINE_NUM_DIGITS: usize = 5;
/// Separator between line number and content. Three terminal columns wide.
const LINE_NUM_SEPARATOR: &str = " \u{2502} ";
const LINE_NUM_SEPARATOR_WIDTH: usize = 3;
/// Full prefix width, e.g. "NNNNN │ " = 8 columns.
const LINE_NUM_PREFIX_WIDTH: usize = LINE_NUM_DIGITS + LINE_NUM_SEPARATOR_WIDTH;

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
    let mut follow_tail = hooks.use_state(|| true);
    let mut scroll_anchor = hooks.use_state(|| None::<ScrollAnchor>);

    // Selection state. Coordinates are (log_line_idx, visual_col) where
    // visual_col is a logical character offset within the log line, regardless
    // of how it wraps onto multiple visual rows.
    let mut selection_anchor = hooks.use_state(|| None::<(usize, usize)>);
    let mut selection_cursor = hooks.use_state(|| None::<(usize, usize)>);
    let mut is_selecting = hooks.use_state(|| false);

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

    let viewport_height = calculate_viewport_height(height);
    let content_width = content_width_for(width);

    // Memoize the wrap layout: rebuilding it on every render walks the entire
    // log buffer, and we'd clone the result again for event handlers. The Arc
    // pointer of `state.logs` is swapped when the buffer changes (Arc::make_mut
    // in handle_activity_log), so (ptr, content_width) is a sufficient key.
    let visual_rows: Arc<Vec<VisualRow>> = hooks.use_memo(
        || Arc::new(build_visual_rows(&state.logs, content_width)),
        (Arc::as_ptr(&state.logs) as usize, content_width),
    );
    let total_visual_rows = visual_rows.len();
    let current_selection_anchor = selection_anchor.get();
    let current_selection_cursor = selection_cursor.get();
    let has_selection = current_selection_anchor.is_some() && current_selection_cursor.is_some();

    let logs_for_copy = state.logs.clone();
    let visual_rows_for_events = visual_rows.clone();

    // Handle keyboard and mouse events - NO MODEL LOCK, only local state updates
    hooks.use_terminal_events({
        let ui_state = ui_state.clone();
        let shutdown = shutdown.clone();
        let command_tx = command_tx.clone();
        let notify = notify.clone();
        let attached = config.attached.load(std::sync::atomic::Ordering::Relaxed);
        move |event| match event {
            TerminalEvent::Key(key_event) => {
                if key_event.kind == KeyEventKind::Release {
                    return;
                }
                handle_key_event(
                    key_event,
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
                if !prompt_active {
                    handle_mouse_event(
                        mouse_event,
                        &mut scroll_offset,
                        &mut follow_tail,
                        &mut scroll_anchor,
                        state.buffer_start_line,
                        total_visual_rows,
                        viewport_height,
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
    let should_exit = exit_flag.is_set()
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

    render_expanded_view(
        &state,
        &visual_rows,
        width,
        height,
        selection.as_ref(),
        ScrollMode {
            follow_tail: follow_tail.get(),
            anchor: scroll_anchor.get(),
        },
        (interrupt_prompt_active, interrupt_prompt_attached),
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
fn calculate_viewport_height(terminal_height: u16) -> usize {
    (terminal_height as usize).saturating_sub(2) // header + footer
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

/// Handle keyboard input - updates local scroll state, no model lock needed
#[allow(clippy::too_many_arguments)]
fn handle_key_event(
    key_event: KeyEvent,
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
    sel: &mut SelectionState<'_>,
) {
    if handle_interrupt_prompt_key(&key_event, ui_state, shutdown, command_tx) {
        return;
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

    if let Some(action) = keyboard_scroll_action(&key_event, viewport_height) {
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

    match key_event.code {
        // Esc: clear selection first, then exit
        KeyCode::Esc => {
            if sel.has_selection {
                sel.clear();
            } else if let Ok(mut ui) = ui_state.write() {
                ui.view_mode = ViewMode::Main;
            }
        }

        // q: always exit
        KeyCode::Char('q') => {
            if let Ok(mut ui) = ui_state.write() {
                ui.view_mode = ViewMode::Main;
            }
        }

        KeyCode::Char('e') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Ok(mut ui) = ui_state.write() {
                ui.view_mode = ViewMode::Main;
            }
        }

        KeyCode::Char('y')
            if !key_event.modifiers.contains(KeyModifiers::CONTROL)
                && !key_event.modifiers.contains(KeyModifiers::ALT) =>
        {
            let selection = match (sel.anchor, sel.cursor) {
                (Some(anchor), Some(cursor)) => Some(Selection::from_anchor_cursor(anchor, cursor)),
                _ => None,
            };
            let text = text_for_yank(sel.logs, sel.buffer_start_line, selection.as_ref());
            if !text.is_empty() {
                copy_to_clipboard(&text);
            }
            if sel.has_selection {
                sel.clear();
            }
        }

        // Ctrl+C: copy selection if active, otherwise open the quit prompt
        KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
            if sel.has_selection {
                if let (Some(anchor), Some(cursor)) = (sel.anchor, sel.cursor) {
                    let selection = Selection::from_anchor_cursor(anchor, cursor);
                    let text = extract_selected_text(sel.logs, sel.buffer_start_line, &selection);
                    if !text.is_empty() {
                        copy_to_clipboard(&text);
                    }
                }
                sel.clear();
            } else if !request_interrupt_prompt(command_tx, ui_state, attached) {
                shutdown.handle_interrupt();
            }
        }

        _ => {}
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
    let map_to_logical = |row: usize, col: usize| -> Option<(usize, usize)> {
        let visible_row_idx = row.checked_sub(1)?;
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

            // Row 0 is header, last row is footer - ignore those
            if row == 0 || row > viewport_height {
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

            // Clamp row to content area
            let clamped_row = row.clamp(1, viewport_height);
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

/// Render the expanded view UI
fn render_expanded_view(
    state: &ExpandedViewState,
    visual_rows: &[VisualRow],
    width: u16,
    height: u16,
    selection: Option<&Selection>,
    scroll_mode: ScrollMode,
    interrupt_prompt: (bool, bool),
) -> AnyElement<'static> {
    let (interrupt_prompt_active, interrupt_prompt_attached) = interrupt_prompt;
    let viewport_height = calculate_viewport_height(height);
    let total_rows = visual_rows.len();

    let clamped_offset = resolved_scroll_offset(
        state.scroll_offset,
        scroll_mode.follow_tail,
        scroll_mode.anchor,
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
        selection,
    );
    let padding_elements = build_padding_elements(visible_rows.len(), viewport_height);

    let mut content_elements = line_elements;
    content_elements.extend(padding_elements);

    let progress = build_progress_indicator(clamped_offset, viewport_height, total_rows);
    let follow_status = if scroll_mode.follow_tail {
        "FOLLOWING"
    } else {
        "PAUSED"
    };

    // Build footer text - show copy hint when selection is active
    let footer_text = if interrupt_prompt_active {
        if interrupt_prompt_attached {
            format!(
                "{} \u{2502} {} \u{2502} Detach or stop the process manager?  Ctrl-C:detach  s:stop manager  Esc:keep watching",
                follow_status, progress
            )
        } else if width < 88 {
            format!(
                "{} \u{2502} {} \u{2502} Quit devenv?  c:keep running  q/Ctrl-C:quit",
                follow_status, progress
            )
        } else {
            format!(
                "{} \u{2502} {} \u{2502} Quit devenv? Nothing has been stopped yet  c:keep running  q:quit  Ctrl-C:quit",
                follow_status, progress
            )
        }
    } else if selection.is_some() {
        format!(
            "{} \u{2502} {} \u{2502} y:copy  j/k:line  ^D/^U:half  ^F/^B:page  g:top  G:follow  Ctrl-C:copy  Esc:deselect  q:back",
            follow_status, progress
        )
    } else {
        format!(
            "{} \u{2502} {} \u{2502} y:copy all  j/k:line  ^D/^U:half  ^F/^B:page  g:top  G:follow  q:back",
            follow_status, progress
        )
    };

    element! {
        View(
            flex_direction: FlexDirection::Column,
            height: height as u32,
            width: width as u32
        ) {
            // Header
            View(height: 1, padding_left: 1, padding_right: 1) {
                Text(
                    content: format!("\u{2500}\u{2500}\u{2500} {} \u{2500}\u{2500}\u{2500}", state.activity_name),
                    color: Color::Cyan,
                    weight: Weight::Bold
                )
            }
            // Log content
            View(flex_grow: 1.0, flex_direction: FlexDirection::Column) {
                #(content_elements)
            }
            // Footer
            View(height: 1, padding_left: 1, padding_right: 1) {
                Text(
                    content: footer_text,
                    color: Color::AnsiValue(245)
                )
            }
        }
    }
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
        let visual_rows = build_visual_rows(&state.logs, content_width_for(100));

        let mut following = render_expanded_view(
            &state,
            &visual_rows,
            100,
            8,
            None,
            ScrollMode {
                follow_tail: true,
                anchor: None,
            },
            (false, false),
        );
        let following_output = following.render(Some(100)).to_string();
        assert!(following_output.contains("FOLLOWING"));
        assert!(following_output.contains("y:copy all"));

        let mut paused = render_expanded_view(
            &state,
            &visual_rows,
            100,
            8,
            None,
            ScrollMode {
                follow_tail: false,
                anchor: None,
            },
            (false, false),
        );
        let paused_output = paused.render(Some(100)).to_string();
        assert!(paused_output.contains("PAUSED"));
        assert!(paused_output.contains("G:follow"));
    }

    #[test]
    fn test_yank_uses_selection_or_complete_log_buffer() {
        let logs = VecDeque::from(["first".to_string(), "second".to_string()]);
        assert_eq!(text_for_yank(&logs, 10, None), "first\nsecond");

        let selection = Selection::from_anchor_cursor((10, 1), (11, 3));
        assert_eq!(text_for_yank(&logs, 10, Some(&selection)), "irst\nsec");
    }

    #[test]
    fn test_render_expanded_view_interrupt_prompt_footer() {
        let state = ExpandedViewState {
            activity_name: "api".to_string(),
            scroll_offset: 0,
            logs: Arc::new(VecDeque::new()),
            buffer_start_line: 0,
        };
        let visual_rows = build_visual_rows(&state.logs, content_width_for(100));

        let mut element = render_expanded_view(
            &state,
            &visual_rows,
            100,
            8,
            None,
            ScrollMode {
                follow_tail: true,
                anchor: None,
            },
            (true, false),
        );
        let output = element.render(Some(100)).to_string();

        assert!(output.contains("Quit devenv? Nothing has been stopped yet"));
        assert!(output.contains("c:keep running"));
        assert!(output.contains("q:quit"));
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
            None,
            ScrollMode {
                follow_tail: false,
                anchor: None,
            },
            (false, false),
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
            Some(&selection),
            ScrollMode {
                follow_tail: false,
                anchor: None,
            },
            (false, false),
        );
        let output = element.render(Some(width as usize)).to_string();
        assert!(output.contains("cdefghij"));
        assert!(output.contains("KLM"));

        let extracted = extract_selected_text(&logs_for_extract, 0, &selection);
        assert_eq!(extracted, "cdefghijKLM");
    }
}
