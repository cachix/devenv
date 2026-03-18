//! Expanded log view component.
//!
//! This component displays build logs in a fullscreen view using the alternate screen buffer.
//! It provides scrollable access to all log lines for a selected activity.
//! Scroll offset is managed as component-local state for immediate responsiveness.
//! Supports mouse-based text selection with OSC 52 clipboard copy.

use crate::TuiConfig;
use crate::app::{ExitFlag, handle_interrupt_prompt_key, request_interrupt_prompt};
use crate::components::{COLOR_HIERARCHY, COLOR_INTERACTIVE};
use crate::input;
use crate::model::{ActivityModel, UiState, ViewMode};
use base64::Engine;
use crossterm::event::MouseButton;
use devenv_processes::ProcessCommand;
use iocraft::prelude::*;
use iocraft::{FullscreenMouseEvent, MouseEventKind};
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::sync::{Arc, RwLock};
use tokio::sync::{Notify, mpsc};
use tokio_shutdown::Shutdown;

/// Width of the non-selectable line number gutter: "NNNNN " = 6 chars.
const LINE_NUM_GUTTER_WIDTH: usize = 6;

/// Column where log content starts: gutter + left border + padding.
const LINE_NUM_PREFIX_WIDTH: usize = LINE_NUM_GUTTER_WIDTH + 2;
fn content_column(col: usize) -> usize {
    col.max(LINE_NUM_PREFIX_WIDTH)
        .saturating_sub(LINE_NUM_PREFIX_WIDTH)
}

/// Represents a normalized text selection range.
struct Selection {
    /// (log_line_index, visual_col), always <= end
    start: (usize, usize),
    /// (log_line_index, visual_col), always >= start
    end: (usize, usize),
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

    /// Returns true if the selection range contains the given (line_idx, col_idx).
    fn contains(&self, line_idx: usize, col_idx: usize) -> bool {
        if line_idx < self.start.0 || line_idx > self.end.0 {
            return false;
        }
        let start_col = if line_idx == self.start.0 {
            self.start.1
        } else {
            0
        };
        let end_col = if line_idx == self.end.0 {
            self.end.1
        } else {
            usize::MAX
        };
        col_idx >= start_col && col_idx < end_col
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
    let notify = hooks.use_context::<Arc<Notify>>().clone();
    let shutdown = hooks.use_context::<Arc<Shutdown>>();
    let command_tx = hooks.use_context::<Option<mpsc::Sender<ProcessCommand>>>();
    let (width, height) = hooks.use_terminal_size();

    // Component-local scroll state - updates are immediate, no model lock needed
    let mut scroll_offset = hooks.use_state(|| 0usize);
    let mut follow_logs = hooks.use_state(|| true);

    // Selection state
    let mut selection_anchor = hooks.use_state(|| None::<(usize, usize)>);
    let mut selection_cursor = hooks.use_state(|| None::<(usize, usize)>);
    let mut is_selecting = hooks.use_state(|| false);

    // Redraw when notified of activity model changes (throttled)
    // This handles new log lines being added
    let redraw = hooks.use_state(|| 0u64);
    hooks.use_future({
        let notify = notify.clone();
        let max_fps = config.max_fps;
        async move {
            crate::throttled_notify_loop(notify, redraw, max_fps).await;
        }
    });

    let mut prev_size = hooks.use_state(crate::TerminalSize::default);
    let current_size = crate::TerminalSize { width, height };
    if current_size != prev_size.get() {
        prev_size.set(current_size);
        if let Ok(mut ui) = ui_state.write() {
            ui.set_terminal_size(current_size.width, current_size.height);
        }
        if let Ok(mut model) = activity_model.write() {
            model.resize_vt(activity_id, current_size.width, current_size.height);
        }
        if let Some(tx) = command_tx.as_ref() {
            let _ = tx.try_send(ProcessCommand::Resize {
                cols: current_size.width,
                rows: current_size.height,
            });
        }
    }

    // Extract view state from activity model (read-only, brief lock)
    let view_state = {
        let model_guard = activity_model.read().unwrap();
        extract_view_state(&model_guard, activity_id, scroll_offset.get())
    };

    let Some(state) = view_state else {
        if let Ok(mut ui) = ui_state.write() {
            ui.view_mode = ViewMode::Main;
        }
        return element!(View).into_any();
    };

    let viewport_height = calculate_viewport_height(height);
    let total_lines = state.total_lines;
    let max_offset = total_lines.saturating_sub(viewport_height);
    let effective_scroll_offset = if follow_logs.get() {
        max_offset
    } else {
        state.scroll_offset.min(max_offset)
    };

    if effective_scroll_offset != scroll_offset.get() {
        scroll_offset.set(effective_scroll_offset);
    }

    let current_selection_anchor = selection_anchor.get();
    let current_selection_cursor = selection_cursor.get();
    let has_selection = current_selection_anchor.is_some() && current_selection_cursor.is_some();
    let input_focused = ui_state
        .read()
        .map(|ui| ui.focused_activity == Some(activity_id))
        .unwrap_or(false);

    let logs_for_copy = state.logs.clone();
    let vt_for_copy = state.vt.clone();
    let activity_name = state.activity_name.clone();

    hooks.use_terminal_events({
        let ui_state = ui_state.clone();
        let notify = notify.clone();
        let shutdown = shutdown.clone();
        let command_tx = command_tx.clone();
        let vt_for_copy = vt_for_copy.clone();
        move |event| match event {
            TerminalEvent::Key(key_event) => {
                if key_event.kind == KeyEventKind::Release {
                    return;
                }
                if !input_focused && state.supports_input && input::is_input_toggle(&key_event) {
                    if let Ok(mut ui) = ui_state.write() {
                        ui.focused_activity = Some(activity_id);
                    }
                    notify.notify_one();
                    return;
                }
                if input_focused {
                    if input::is_input_toggle(&key_event) {
                        if let Ok(mut ui) = ui_state.write()
                            && ui.focused_activity == Some(activity_id)
                        {
                            ui.focused_activity = None;
                        }
                        notify.notify_one();
                        return;
                    }

                    if let Some(tx) = command_tx.as_ref()
                        && let Some(data) = input::encode_key_event(&key_event)
                    {
                        let _ = tx.try_send(ProcessCommand::SendInput { activity_id, data });
                        notify.notify_one();
                    }
                    return;
                }

                handle_key_event(
                    key_event,
                    &activity_name,
                    state.supports_input,
                    &ui_state,
                    &notify,
                    &shutdown,
                    command_tx.as_ref(),
                    &mut scroll_offset,
                    &mut follow_logs,
                    total_lines,
                    viewport_height,
                    &mut SelectionState {
                        has_selection,
                        anchor: current_selection_anchor,
                        cursor: current_selection_cursor,
                        logs: &logs_for_copy,
                        vt: vt_for_copy.as_ref(),
                        anchor_state: &mut selection_anchor,
                        cursor_state: &mut selection_cursor,
                        is_selecting: &mut is_selecting,
                    },
                );
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
                        &mut follow_logs,
                        total_lines,
                        viewport_height,
                        &logs_for_copy,
                        vt_for_copy.as_ref(),
                        &mut selection_anchor,
                        &mut selection_cursor,
                        &mut is_selecting,
                    );
                }
            }
            TerminalEvent::Resize(_, _) | _ => {}
        }
    });

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

    let selection =
        if let (Some(anchor), Some(cursor)) = (selection_anchor.get(), selection_cursor.get()) {
            Some(Selection::from_anchor_cursor(anchor, cursor))
        } else {
            None
        };

    let interrupt_prompt_active = ui_state
        .read()
        .map(|ui| ui.interrupt_prompt_active())
        .unwrap_or(false);

    let state = ExpandedViewState {
        scroll_offset: effective_scroll_offset,
        ..state
    };

    render_expanded_view(
        &state,
        width,
        height,
        selection.as_ref(),
        interrupt_prompt_active,
        shutdown.is_cancelled(),
        follow_logs.get(),
        state.supports_input,
        input_focused,
    )
}

/// State extracted from the model for rendering
struct ExpandedViewState {
    activity_name: String,
    scroll_offset: usize,
    logs: Arc<VecDeque<String>>,
    total_lines: usize,
    supports_input: bool,
    vt: Option<Arc<std::sync::Mutex<avt::Vt>>>,
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

fn trim_trailing_blank_vt_lines<'a>(lines: &'a [&'a avt::Line]) -> &'a [&'a avt::Line] {
    let Some(last_content_idx) = lines
        .iter()
        .rposition(|line| !is_blank_vt_row(line.cells()))
    else {
        return &[];
    };

    &lines[..=last_content_idx]
}

fn vt_line_is_wrapped(line: &avt::Line) -> bool {
    format!("{line:?}").contains('⏎')
}

fn vt_display_line_numbers(lines: &[&avt::Line]) -> Vec<usize> {
    let mut numbers = Vec::with_capacity(lines.len());
    let mut current = 1;

    for (idx, _) in lines.iter().enumerate() {
        if idx > 0 && !vt_line_is_wrapped(lines[idx - 1]) {
            current += 1;
        }
        numbers.push(current);
    }

    numbers
}

fn extract_view_state(
    model: &ActivityModel,
    activity_id: u64,
    scroll_offset: usize,
) -> Option<ExpandedViewState> {
    let activity = model.get_activity(activity_id)?;
    let activity_name = activity.name.clone();
    let supports_input = matches!(activity.variant, crate::model::ActivityVariant::Process(_));
    let vt = activity.vt.clone();

    let logs = model
        .get_build_logs(activity_id)
        .cloned()
        .unwrap_or_else(|| Arc::new(VecDeque::new()));

    let total_lines = if let Some(vt) = &vt {
        let vt = vt.lock().unwrap();
        let rows: Vec<&[avt::Cell]> = vt.lines().map(|row| row.cells()).collect();
        trim_trailing_blank_vt_rows(&rows).len()
    } else {
        logs.len()
    };

    Some(ExpandedViewState {
        activity_name,
        scroll_offset,
        logs,
        total_lines,
        supports_input,
        vt,
    })
}

fn calculate_viewport_height(terminal_height: u16) -> usize {
    (terminal_height as usize).saturating_sub(2)
}

struct SelectionState<'a> {
    has_selection: bool,
    anchor: Option<(usize, usize)>,
    cursor: Option<(usize, usize)>,
    logs: &'a Arc<VecDeque<String>>,
    vt: Option<&'a Arc<std::sync::Mutex<avt::Vt>>>,
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

    fn has_selection(&self) -> bool {
        self.has_selection
    }

    fn copy_current_selection(&self) {
        if let (Some(anchor), Some(cursor)) = (self.anchor, self.cursor) {
            let selection = Selection::from_anchor_cursor(anchor, cursor);
            let text = extract_selected_text(self.logs, self.vt, &selection);
            if !text.is_empty() {
                copy_to_clipboard(&text);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_key_event(
    key_event: KeyEvent,
    activity_name: &str,
    supports_input: bool,
    ui_state: &Arc<RwLock<UiState>>,
    notify: &Arc<Notify>,
    shutdown: &Arc<Shutdown>,
    command_tx: Option<&mpsc::Sender<ProcessCommand>>,
    scroll_offset: &mut State<usize>,
    follow_logs: &mut State<bool>,
    total_lines: usize,
    viewport_height: usize,
    sel: &mut SelectionState<'_>,
) {
    if handle_interrupt_prompt_key(&key_event, ui_state, shutdown) {
        notify.notify_one();
        return;
    }

    if shutdown.is_cancelled() {
        return;
    }

    let max_offset = total_lines.saturating_sub(viewport_height);

    let set_scroll =
        |scroll_offset: &mut State<usize>, follow_logs: &mut State<bool>, new_offset: usize| {
            let clamped = new_offset.min(max_offset);
            scroll_offset.set(clamped);
            follow_logs.set(clamped == max_offset);
        };

    match key_event.code {
        KeyCode::Esc => {
            if sel.has_selection() {
                sel.clear();
            } else if let Ok(mut ui) = ui_state.write() {
                ui.view_mode = ViewMode::Main;
                notify.notify_one();
            }
        }
        KeyCode::Char('q') => {
            if let Ok(mut ui) = ui_state.write() {
                ui.view_mode = ViewMode::Main;
                notify.notify_one();
            }
        }
        KeyCode::Char('e') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Ok(mut ui) = ui_state.write() {
                ui.view_mode = ViewMode::Main;
                notify.notify_one();
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            set_scroll(scroll_offset, follow_logs, scroll_offset.get() + 1);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            set_scroll(
                scroll_offset,
                follow_logs,
                scroll_offset.get().saturating_sub(1),
            );
        }
        KeyCode::PageDown | KeyCode::Char(' ') => {
            set_scroll(
                scroll_offset,
                follow_logs,
                scroll_offset.get() + viewport_height,
            );
        }
        KeyCode::PageUp => {
            set_scroll(
                scroll_offset,
                follow_logs,
                scroll_offset.get().saturating_sub(viewport_height),
            );
        }
        KeyCode::Home | KeyCode::Char('g') => {
            set_scroll(scroll_offset, follow_logs, 0);
        }
        KeyCode::End | KeyCode::Char('G') => {
            set_scroll(scroll_offset, follow_logs, max_offset);
        }
        KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
            if sel.has_selection() {
                sel.copy_current_selection();
                sel.clear();
            } else if !request_interrupt_prompt(command_tx, ui_state) {
                shutdown.handle_interrupt();
            }
            notify.notify_one();
        }
        KeyCode::Char('r') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(tx) = command_tx {
                let _ = tx.try_send(ProcessCommand::Restart(activity_name.to_string()));
                notify.notify_one();
            }
        }
        KeyCode::Char('y') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
            if supports_input && let Some(tx) = command_tx {
                let _ = tx.try_send(ProcessCommand::Stop(activity_name.to_string()));
                notify.notify_one();
            }
        }
        _ => {}
    }
}

fn handle_mouse_event(
    mouse_event: FullscreenMouseEvent,
    scroll_offset: &mut State<usize>,
    follow_logs: &mut State<bool>,
    total_lines: usize,
    viewport_height: usize,
    logs: &Arc<VecDeque<String>>,
    vt: Option<&Arc<std::sync::Mutex<avt::Vt>>>,
    selection_anchor: &mut State<Option<(usize, usize)>>,
    selection_cursor: &mut State<Option<(usize, usize)>>,
    is_selecting: &mut State<bool>,
) {
    let scroll_lines = 3;
    let max_offset = total_lines.saturating_sub(viewport_height);

    let set_scroll =
        |scroll_offset: &mut State<usize>, follow_logs: &mut State<bool>, new_offset: usize| {
            let clamped = new_offset.min(max_offset);
            scroll_offset.set(clamped);
            follow_logs.set(clamped == max_offset);
        };

    match mouse_event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let row = mouse_event.row as usize;
            let col = mouse_event.column as usize;

            if row == 0 || row > viewport_height {
                return;
            }

            let visible_line_idx = row - 1;
            let log_line_idx = scroll_offset.get() + visible_line_idx;

            if log_line_idx >= total_lines {
                selection_anchor.set(None);
                selection_cursor.set(None);
                is_selecting.set(true);
                return;
            }

            let visual_col = content_column(col);

            let pos = (log_line_idx, visual_col);
            selection_anchor.set(Some(pos));
            selection_cursor.set(Some(pos));
            is_selecting.set(true);
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if !is_selecting.get() {
                return;
            }

            let row = mouse_event.row as usize;
            let col = mouse_event.column as usize;

            let clamped_row = row.clamp(1, viewport_height);
            let visible_line_idx = clamped_row - 1;
            if total_lines == 0 {
                return;
            }

            let log_line_idx =
                (scroll_offset.get() + visible_line_idx).min(total_lines.saturating_sub(1));

            let visual_col = content_column(col);
            if selection_anchor.get().is_some() {
                selection_cursor.set(Some((log_line_idx, visual_col)));
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            is_selecting.set(false);

            if let (Some(anchor), Some(cursor)) = (selection_anchor.get(), selection_cursor.get()) {
                if anchor != cursor {
                    let selection = Selection::from_anchor_cursor(anchor, cursor);
                    let text = extract_selected_text(logs, vt, &selection);
                    copy_to_clipboard(&text);
                }
            }

            selection_anchor.set(None);
            selection_cursor.set(None);
        }
        MouseEventKind::ScrollDown => {
            set_scroll(
                scroll_offset,
                follow_logs,
                scroll_offset.get() + scroll_lines,
            );
        }
        MouseEventKind::ScrollUp => {
            set_scroll(
                scroll_offset,
                follow_logs,
                scroll_offset.get().saturating_sub(scroll_lines),
            );
        }
        _ => {}
    }
}

fn copy_to_clipboard(text: &str) {
    if text.is_empty() {
        return;
    }

    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    if let Ok(mut tty) = OpenOptions::new().write(true).open("/dev/tty") {
        write_osc52_sequence(&mut tty, &encoded);
    } else {
        let mut stdout = std::io::stdout();
        write_osc52_sequence(&mut stdout, &encoded);
    }
}

fn write_osc52_sequence<W: std::io::Write>(out: &mut W, encoded: &str) {
    if std::env::var_os("TMUX").is_some() {
        let _ = write!(out, "\x1bPtmux;\x1b\x1b]52;c;{}\x07\x1b\\", encoded);
    } else {
        let _ = write!(out, "\x1b]52;c;{}\x07", encoded);
    }
    let _ = out.flush();
}

fn extract_selected_text(
    logs: &VecDeque<String>,
    vt: Option<&Arc<std::sync::Mutex<avt::Vt>>>,
    selection: &Selection,
) -> String {
    let mut lines = Vec::new();

    if let Some(vt) = vt {
        let vt = vt.lock().unwrap();
        let rows: Vec<&[avt::Cell]> = vt.lines().map(|row| row.cells()).collect();
        let rows = trim_trailing_blank_vt_rows(&rows);
        for line_idx in selection.start.0..=selection.end.0 {
            let Some(row) = rows.get(line_idx) else {
                continue;
            };
            let len = row.len();
            if let Some((start_col, end_col)) = selection.line_range(line_idx, len) {
                let mut line_text = String::new();
                for col in start_col..end_col {
                    if let Some(cell) = row.get(col) {
                        line_text.push(cell.char());
                    }
                }
                lines.push(line_text);
            }
        }
    } else {
        for line_idx in selection.start.0..=selection.end.0 {
            let Some(line) = logs.get(line_idx) else {
                continue;
            };

            if let Some((start_col, end_col)) = selection.line_range(line_idx, line.len()) {
                let selected: String = line
                    .chars()
                    .skip(start_col)
                    .take(end_col - start_col)
                    .collect();
                lines.push(selected);
            }
        }
    }

    lines.join("\n")
}

fn render_expanded_view(
    state: &ExpandedViewState,
    width: u16,
    height: u16,
    selection: Option<&Selection>,
    interrupt_prompt_active: bool,
    shutting_down: bool,
    follow_logs: bool,
    supports_input: bool,
    input_focused: bool,
) -> AnyElement<'static> {
    let viewport_height = calculate_viewport_height(height);
    let content_width = (width as usize).saturating_sub(LINE_NUM_PREFIX_WIDTH);

    let (gutter_elements, content_elements, progress) = if let Some(vt) = &state.vt {
        let vt = vt.lock().unwrap();
        let mut gutter_elements = Vec::new();
        let mut line_elements = Vec::new();
        let lines: Vec<&avt::Line> = vt.lines().collect();
        let lines = trim_trailing_blank_vt_lines(&lines);
        let display_numbers = vt_display_line_numbers(lines);
        let visible_lines = lines.iter().skip(state.scroll_offset).take(viewport_height);

        for (i, line) in visible_lines.enumerate() {
            let log_line_idx = state.scroll_offset + i;
            let line_num = display_numbers
                .get(log_line_idx)
                .copied()
                .unwrap_or(log_line_idx + 1);
            let line_gutter = format!("{:>5} ", line_num);

            gutter_elements.push(
                element! {
                    View(height: 1, width: LINE_NUM_GUTTER_WIDTH as u32, flex_shrink: 0.0) {
                        Text(
                            content: line_gutter,
                            color: Color::AnsiValue(250),
                            wrap: TextWrap::NoWrap
                        )
                    }
                }
                .into_any(),
            );

            line_elements.push(build_vt_line_element(
                line.cells(),
                log_line_idx,
                content_width,
                selection,
            ));
        }
        let total = line_elements.len();
        let padding = build_padding_elements(total, viewport_height);
        gutter_elements.extend(build_gutter_padding_elements(total, viewport_height));
        line_elements.extend(padding);
        (
            gutter_elements,
            line_elements,
            build_progress_indicator(state.scroll_offset, viewport_height, lines.len()),
        )
    } else {
        let max_offset = state.total_lines.saturating_sub(viewport_height);
        let clamped_offset = state.scroll_offset.min(max_offset);

        let visible_lines: Vec<&String> = state
            .logs
            .iter()
            .skip(clamped_offset)
            .take(viewport_height)
            .collect();

        let (mut gutter_elements, mut line_elements) =
            build_line_elements(&visible_lines, clamped_offset, width, selection);

        gutter_elements.extend(build_gutter_padding_elements(
            visible_lines.len(),
            viewport_height,
        ));
        let padding_elements = build_padding_elements(visible_lines.len(), viewport_height);
        line_elements.extend(padding_elements);

        (
            gutter_elements,
            line_elements,
            build_progress_indicator(clamped_offset, viewport_height, state.total_lines),
        )
    };

    let mode = if follow_logs { "follow" } else { "scrolled" };
    let use_symbols = width < 60;
    let use_short_text = width < 100;

    let interrupt_footer_text = if interrupt_prompt_active {
        Some(if width < 88 {
            format!("{} │ Quit devenv?  c:keep running  q/^C:quit", progress)
        } else {
            format!(
                "{} │ Quit devenv? Nothing has been stopped yet  c:keep running  q:quit  ^C:quit",
                progress
            )
        })
    } else {
        None
    };

    let mut footer_left = vec![
        element!(Text(content: progress, color: Color::AnsiValue(245))).into_any(),
        element!(Text(content: " ")).into_any(),
        element!(Text(content: "|", color: COLOR_HIERARCHY)).into_any(),
        element!(Text(content: " ")).into_any(),
    ];

    if !interrupt_prompt_active {
        footer_left.push(
            element!(Text(content: mode, color: COLOR_INTERACTIVE, weight: Weight::Bold))
                .into_any(),
        );

        if input_focused {
            footer_left.push(element!(Text(content: " ")).into_any());
            footer_left.push(element!(Text(content: "|", color: COLOR_HIERARCHY)).into_any());
            footer_left.push(element!(Text(content: " ")).into_any());
            footer_left.push(
                element!(Text(content: "input captured", color: Color::AnsiValue(214))).into_any(),
            );
        }
    }

    let mut footer_right = vec![];
    if !interrupt_prompt_active && selection.is_some() {
        footer_right.push(element!(Text(content: "^C", color: COLOR_INTERACTIVE)).into_any());
        footer_right.push(element!(Text(content: " copy • ")).into_any());
        footer_right.push(element!(Text(content: "Esc", color: COLOR_INTERACTIVE)).into_any());
        footer_right.push(element!(Text(content: " deselect • ")).into_any());
        footer_right.push(element!(Text(content: "q", color: COLOR_INTERACTIVE)).into_any());
        footer_right.push(element!(Text(content: " back")).into_any());
    } else if !interrupt_prompt_active {
        footer_right.push(element!(Text(content: "↑", color: COLOR_INTERACTIVE)).into_any());
        footer_right.push(element!(Text(content: "↓", color: COLOR_INTERACTIVE)).into_any());
        if !use_symbols {
            footer_right.push(
                element!(Text(content: if use_short_text { " scroll • " } else { " scroll logs • " }))
                    .into_any(),
            );
        } else {
            footer_right.push(element!(Text(content: " • ")).into_any());
        }
        footer_right.push(element!(Text(content: "PgUp", color: COLOR_INTERACTIVE)).into_any());
        footer_right.push(element!(Text(content: "/", color: COLOR_HIERARCHY)).into_any());
        footer_right.push(element!(Text(content: "PgDn", color: COLOR_INTERACTIVE)).into_any());
        footer_right.push(
            element!(Text(content: if use_short_text { " page • " } else { " page jump • " }))
                .into_any(),
        );
        footer_right.push(element!(Text(content: "g", color: COLOR_INTERACTIVE)).into_any());
        footer_right.push(element!(Text(content: "/", color: COLOR_HIERARCHY)).into_any());
        footer_right.push(element!(Text(content: "G", color: COLOR_INTERACTIVE)).into_any());
        footer_right.push(
            element!(Text(content: if use_short_text { " ends" } else { " top-bottom" }))
                .into_any(),
        );

        if supports_input && !shutting_down {
            footer_right.push(element!(Text(content: " • ")).into_any());
            footer_right.push(element!(Text(content: "^r", color: COLOR_INTERACTIVE)).into_any());
            footer_right.push(
                element!(Text(content: if use_short_text { " restart" } else { " restart process" }))
                    .into_any(),
            );

            footer_right.push(element!(Text(content: " • ")).into_any());
            footer_right.push(element!(Text(content: "^y", color: COLOR_INTERACTIVE)).into_any());
            footer_right.push(
                element!(Text(content: if use_short_text { " stop" } else { " stop process" }))
                    .into_any(),
            );

            footer_right.push(element!(Text(content: " • ")).into_any());
            footer_right.push(
                element!(Text(content: input::INPUT_TOGGLE_HINT, color: COLOR_INTERACTIVE))
                    .into_any(),
            );
            footer_right.push(
                element!(Text(content: if input_focused { " return" } else { " input" }))
                    .into_any(),
            );
        }

        footer_right.push(element!(Text(content: " • ")).into_any());
        footer_right.push(element!(Text(content: "q", color: COLOR_INTERACTIVE)).into_any());
        footer_right.push(element!(Text(content: " back")).into_any());
    }

    element! {
        View(
            flex_direction: FlexDirection::Column,
            height: height as u32,
            width: width as u32
        ) {
            View(height: 1, padding_left: 1, padding_right: 1) {
                Text(
                    content: format!("--- {} ---", state.activity_name),
                    color: Color::Cyan,
                    weight: Weight::Bold
                )
            }
            View(flex_grow: 1.0, flex_direction: FlexDirection::Row) {
                View(width: LINE_NUM_GUTTER_WIDTH as u32, flex_shrink: 0.0, flex_direction: FlexDirection::Column) {
                    #(gutter_elements)
                }
                View(
                    width: content_width as u32,
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Left,
                    border_color: Color::AnsiValue(250),
                    padding_left: 1,
                    overflow: Overflow::Hidden,
                ) {
                    #(content_elements)
                }
            }
            View(height: 1, padding_left: 1, padding_right: 1) {
                #(if let Some(interrupt_footer_text) = interrupt_footer_text {
                    vec![element!(Text(
                        content: interrupt_footer_text,
                        color: Color::AnsiValue(245)
                    )).into_any()]
                } else {
                    vec![element! {
                        View(
                            flex_direction: FlexDirection::Row,
                            justify_content: JustifyContent::SpaceBetween,
                            width: 100pct
                        ) {
                            View(flex_direction: FlexDirection::Row, flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                                #(footer_left)
                            }
                            View(flex_direction: FlexDirection::Row, flex_shrink: 0.0, margin_left: if use_symbols { 1 } else { 2 }) {
                                #(footer_right)
                            }
                        }
                    }.into_any()]
                })
            }
        }
    }
    .into_any()
}

fn build_vt_line_element(
    row: &[avt::Cell],
    log_line_idx: usize,
    content_width: usize,
    selection: Option<&Selection>,
) -> AnyElement<'static> {
    let mut runs: Vec<(String, avt::Pen, bool)> = Vec::new();

    for (col_idx, cell) in row.iter().take(content_width).enumerate() {
        let is_selected = selection.is_some_and(|s| s.contains(log_line_idx, col_idx));

        if let Some((text, pen, selected)) = runs.last_mut()
            && *selected == is_selected
            && *pen == *cell.pen()
        {
            text.push(cell.char());
        } else {
            runs.push((cell.char().to_string(), cell.pen().clone(), is_selected));
        }
    }

    let run_elements = runs
        .into_iter()
        .map(|(text, pen, selected)| build_vt_segment_element(text, &pen, selected))
        .collect::<Vec<_>>();

    element! {
        View(height: 1, width: content_width as u32, overflow: Overflow::Hidden, flex_direction: FlexDirection::Row) {
            #(run_elements)
        }
    }
    .into_any()
}

fn build_vt_segment_element(text: String, pen: &avt::Pen, selected: bool) -> AnyElement<'static> {
    let fg = if selected {
        Some(Color::AnsiValue(232))
    } else {
        pen.foreground().map(avt_color_to_iocraft)
    };

    let bg = if selected {
        Some(Color::AnsiValue(250))
    } else {
        pen.background().map(avt_color_to_iocraft)
    };

    let text = element! {
        Text(
            content: text,
            color: fg,
            weight: if pen.is_bold() { Weight::Bold } else { Weight::Normal },
            italic: pen.is_italic(),
            decoration: if pen.is_underline() { TextDecoration::Underline } else { TextDecoration::None },
            wrap: TextWrap::NoWrap
        )
    }
    .into_any();

    if let Some(background_color) = bg {
        element! {
            View(background_color: background_color) {
                #(vec![text])
            }
        }
        .into_any()
    } else {
        text
    }
}

fn avt_color_to_iocraft(color: avt::Color) -> Color {
    match color {
        avt::Color::Indexed(c) => Color::AnsiValue(c),
        avt::Color::RGB(rgb) => Color::Rgb {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        },
    }
}

fn build_line_elements(
    visible_lines: &[&String],
    offset: usize,
    width: u16,
    selection: Option<&Selection>,
) -> (Vec<AnyElement<'static>>, Vec<AnyElement<'static>>) {
    let mut gutter_elements = Vec::with_capacity(visible_lines.len());
    let mut elements = Vec::with_capacity(visible_lines.len());
    let content_width = (width as usize).saturating_sub(LINE_NUM_PREFIX_WIDTH);

    for (i, line) in visible_lines.iter().enumerate() {
        let line_num = offset + i + 1;
        let log_line_idx = offset + i;

        let display_line = if line.len() > content_width {
            format!("{}...", &line[..content_width.saturating_sub(3)])
        } else {
            (*line).clone()
        };

        let line_gutter = format!("{:>5} ", line_num);
        gutter_elements.push(
            element! {
                View(height: 1, width: LINE_NUM_GUTTER_WIDTH as u32, flex_shrink: 0.0) {
                    Text(
                        content: line_gutter,
                        color: Color::AnsiValue(250),
                        wrap: TextWrap::NoWrap
                    )
                }
            }
            .into_any(),
        );

        let sel_range = selection.and_then(|s| s.line_range(log_line_idx, display_line.len()));
        let line_color = Color::AnsiValue(250);

        if let Some((start_col, end_col)) = sel_range {
            let before: String = display_line.chars().take(start_col).collect();
            let selected: String = display_line
                .chars()
                .skip(start_col)
                .take(end_col - start_col)
                .collect();
            let after: String = display_line.chars().skip(end_col).collect();

            elements.push(
                element! {
                    View(height: 1, width: content_width as u32, overflow: Overflow::Hidden, flex_direction: FlexDirection::Row) {
                        Text(
                            content: before,
                            color: line_color,
                            wrap: TextWrap::NoWrap
                        )
                        View(background_color: Color::AnsiValue(250)) {
                            Text(
                                content: selected,
                                color: Color::AnsiValue(232),
                                wrap: TextWrap::NoWrap
                            )
                        }
                        Text(
                            content: after,
                            color: line_color,
                            wrap: TextWrap::NoWrap
                        )
                    }
                }
                .into_any(),
            );
        } else {
            elements.push(
                element! {
                    View(height: 1, width: content_width as u32, overflow: Overflow::Hidden) {
                        Text(
                            content: display_line,
                            color: line_color,
                            wrap: TextWrap::NoWrap
                        )
                    }
                }
                .into_any(),
            );
        }
    }

    (gutter_elements, elements)
}

fn build_gutter_padding_elements(
    filled_lines: usize,
    viewport_height: usize,
) -> Vec<AnyElement<'static>> {
    let padding_count = viewport_height.saturating_sub(filled_lines);
    let mut elements = Vec::with_capacity(padding_count);

    for _ in 0..padding_count {
        elements.push(
            element! {
                View(height: 1, width: LINE_NUM_GUTTER_WIDTH as u32, flex_shrink: 0.0) {
                    Text(content: "      ".to_string(), wrap: TextWrap::NoWrap)
                }
            }
            .into_any(),
        );
    }

    elements
}

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
    fn test_render_expanded_view_interrupt_prompt_footer() {
        let state = ExpandedViewState {
            activity_name: "api".to_string(),
            scroll_offset: 0,
            logs: Arc::new(VecDeque::new()),
            total_lines: 0,
            supports_input: false,
            vt: None,
        };

        let mut element =
            render_expanded_view(&state, 100, 8, None, true, false, false, false, false);
        let output = element.render(Some(100)).to_string();

        assert!(output.contains("Quit devenv? Nothing has been stopped yet"));
        assert!(output.contains("c:keep running"));
        assert!(output.contains("q:quit"));
    }
}
