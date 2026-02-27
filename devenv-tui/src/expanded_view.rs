//! Expanded log view component.
//!
//! This component displays build logs in a fullscreen view using the alternate screen buffer.
//! It provides scrollable access to all log lines for a selected activity.
//! Scroll offset is managed as component-local state for immediate responsiveness.
//! Supports mouse-based text selection with OSC 52 clipboard copy.

use crate::TuiConfig;
use crate::model::{ActivityModel, UiState, ViewMode};
use base64::Engine;
use crossterm::event::MouseButton;
use iocraft::prelude::*;
use iocraft::{FullscreenMouseEvent, MouseEventKind};
use std::collections::VecDeque;
use std::io::Write as _;
use std::sync::{Arc, RwLock};
use tokio::sync::Notify;
use tokio_shutdown::{Shutdown, Signal};

/// Line number prefix width: "NNNNN | " = 8 chars
const LINE_NUM_PREFIX_WIDTH: usize = 8;

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
    let shutdown = hooks.use_context::<Arc<Shutdown>>();
    let (width, height) = hooks.use_terminal_size();

    // Component-local scroll state - updates are immediate, no model lock needed
    let mut scroll_offset = hooks.use_state(|| 0usize);

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
    let total_lines = state.total_lines;

    // Build current selection for use in event handlers
    let current_selection_anchor = selection_anchor.get();
    let current_selection_cursor = selection_cursor.get();
    let has_selection = current_selection_anchor.is_some() && current_selection_cursor.is_some();

    // Clone logs Arc for use in copy handler
    let logs_for_copy = state.logs.clone();

    // Handle keyboard and mouse events - NO MODEL LOCK, only local state updates
    hooks.use_terminal_events({
        let ui_state = ui_state.clone();
        let shutdown = shutdown.clone();
        move |event| match event {
            TerminalEvent::Key(key_event) => {
                if key_event.kind == KeyEventKind::Release {
                    return;
                }
                handle_key_event(
                    key_event,
                    &ui_state,
                    &shutdown,
                    &mut scroll_offset,
                    total_lines,
                    viewport_height,
                    &mut SelectionState {
                        has_selection,
                        anchor: current_selection_anchor,
                        cursor: current_selection_cursor,
                        logs: &logs_for_copy,
                        anchor_state: &mut selection_anchor,
                        cursor_state: &mut selection_cursor,
                        is_selecting: &mut is_selecting,
                    },
                );
            }
            TerminalEvent::FullscreenMouse(mouse_event) => {
                handle_mouse_event(
                    mouse_event,
                    &mut scroll_offset,
                    total_lines,
                    viewport_height,
                    &mut selection_anchor,
                    &mut selection_cursor,
                    &mut is_selecting,
                );
            }
            TerminalEvent::Resize(_, _) | _ => {}
        }
    });

    // Check if we should exit (view mode changed)
    let should_exit = ui_state
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

    render_expanded_view(&state, width, height, selection.as_ref())
}

/// State extracted from the model for rendering
struct ExpandedViewState {
    activity_name: String,
    scroll_offset: usize,
    logs: Arc<VecDeque<String>>,
    total_lines: usize,
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

    let total_lines = logs.len();

    Some(ExpandedViewState {
        activity_name,
        scroll_offset,
        logs,
        total_lines,
    })
}

/// Calculate the viewport height (total height minus header and footer)
fn calculate_viewport_height(terminal_height: u16) -> usize {
    (terminal_height as usize).saturating_sub(2) // header + footer
}

/// Mutable selection state passed to event handlers.
struct SelectionState<'a> {
    has_selection: bool,
    anchor: Option<(usize, usize)>,
    cursor: Option<(usize, usize)>,
    logs: &'a Arc<VecDeque<String>>,
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
    scroll_offset: &mut State<usize>,
    total_lines: usize,
    viewport_height: usize,
    sel: &mut SelectionState<'_>,
) {
    let max_offset = total_lines.saturating_sub(viewport_height);

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

        // Copy selection with Enter or y
        KeyCode::Enter | KeyCode::Char('y') => {
            if sel.has_selection {
                if let (Some(anchor), Some(cursor)) = (sel.anchor, sel.cursor) {
                    let selection = Selection::from_anchor_cursor(anchor, cursor);
                    let text = extract_selected_text(sel.logs, &selection);
                    if !text.is_empty() {
                        copy_to_clipboard(&text);
                    }
                }
                sel.clear();
            }
        }

        // Scroll down one line
        KeyCode::Down | KeyCode::Char('j') => {
            scroll_offset.set((scroll_offset.get() + 1).min(max_offset));
        }

        // Scroll up one line
        KeyCode::Up | KeyCode::Char('k') => {
            scroll_offset.set(scroll_offset.get().saturating_sub(1));
        }

        // Page down
        KeyCode::PageDown | KeyCode::Char(' ') => {
            scroll_offset.set((scroll_offset.get() + viewport_height).min(max_offset));
        }

        // Page up
        KeyCode::PageUp => {
            scroll_offset.set(scroll_offset.get().saturating_sub(viewport_height));
        }

        // Go to top
        KeyCode::Home | KeyCode::Char('g') => {
            scroll_offset.set(0);
        }

        // Go to bottom
        KeyCode::End | KeyCode::Char('G') => {
            scroll_offset.set(max_offset);
        }

        // Ctrl+C to shutdown
        KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
            shutdown.set_last_signal(Signal::SIGINT);
            shutdown.shutdown();
        }

        _ => {}
    }
}

/// Handle mouse input - updates local scroll state and selection
fn handle_mouse_event(
    mouse_event: FullscreenMouseEvent,
    scroll_offset: &mut State<usize>,
    total_lines: usize,
    viewport_height: usize,
    selection_anchor: &mut State<Option<(usize, usize)>>,
    selection_cursor: &mut State<Option<(usize, usize)>>,
    is_selecting: &mut State<bool>,
) {
    let scroll_lines = 3; // Lines to scroll per wheel tick
    let max_offset = total_lines.saturating_sub(viewport_height);

    match mouse_event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let row = mouse_event.row as usize;
            let col = mouse_event.column as usize;

            // Row 0 is header, last row is footer - ignore those
            if row == 0 || row > viewport_height {
                return;
            }

            // Map to log line index: row 1 = first visible line
            let visible_line_idx = row - 1;
            let log_line_idx = scroll_offset.get() + visible_line_idx;

            if log_line_idx >= total_lines {
                return;
            }

            // Map column to content position (subtract line number prefix)
            let visual_col = col.saturating_sub(LINE_NUM_PREFIX_WIDTH);

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

            // Clamp row to content area
            let clamped_row = row.clamp(1, viewport_height);
            let visible_line_idx = clamped_row - 1;
            let log_line_idx =
                (scroll_offset.get() + visible_line_idx).min(total_lines.saturating_sub(1));

            let visual_col = col.saturating_sub(LINE_NUM_PREFIX_WIDTH);

            selection_cursor.set(Some((log_line_idx, visual_col)));
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
            scroll_offset.set((scroll_offset.get() + scroll_lines).min(max_offset));
        }
        MouseEventKind::ScrollUp => {
            scroll_offset.set(scroll_offset.get().saturating_sub(scroll_lines));
        }
        _ => {}
    }
}

/// Copy text to clipboard using OSC 52 escape sequence.
/// This works in most modern terminals including over SSH.
fn copy_to_clipboard(text: &str) {
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    // Write OSC 52 sequence to the TUI output target.
    // Uses the tty fd when available since fd 2 may be redirected to /dev/null.
    #[cfg(unix)]
    {
        if let Some(fd) = crate::app::tty_fd() {
            let mut writer = crate::app::FdWriter(fd);
            let _ = write!(writer, "\x1b]52;c;{}\x07", encoded);
            let _ = writer.flush();
            return;
        }
    }
    let _ = write!(std::io::stderr(), "\x1b]52;c;{}\x07", encoded);
    let _ = std::io::stderr().flush();
}

/// Extract selected text from log lines, stripping ANSI codes.
fn extract_selected_text(logs: &VecDeque<String>, selection: &Selection) -> String {
    let mut lines = Vec::new();

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

    lines.join("\n")
}

/// Render the expanded view UI
fn render_expanded_view(
    state: &ExpandedViewState,
    width: u16,
    height: u16,
    selection: Option<&Selection>,
) -> AnyElement<'static> {
    let viewport_height = calculate_viewport_height(height);

    // Clamp scroll offset to valid range
    let max_offset = state.total_lines.saturating_sub(viewport_height);
    let clamped_offset = state.scroll_offset.min(max_offset);

    // Get visible lines
    let visible_lines: Vec<&String> = state
        .logs
        .iter()
        .skip(clamped_offset)
        .take(viewport_height)
        .collect();

    // Build line elements
    let line_elements = build_line_elements(&visible_lines, clamped_offset, width, selection);

    // Build empty line padding
    let padding_elements = build_padding_elements(visible_lines.len(), viewport_height);

    // Combine all content elements
    let mut content_elements = line_elements;
    content_elements.extend(padding_elements);

    // Build progress indicator
    let progress = build_progress_indicator(clamped_offset, viewport_height, state.total_lines);

    // Build footer text - show copy hint when selection is active
    let footer_text = if selection.is_some() {
        format!(
            "{} \u{2502} j/k:line  PgUp/PgDn:page  g/G:top/bottom  Enter:copy  Esc:deselect  q:back",
            progress
        )
    } else {
        format!(
            "{} \u{2502} j/k:line  PgUp/PgDn:page  g/G:top/bottom  q:back",
            progress
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

/// Build elements for visible log lines
fn build_line_elements(
    visible_lines: &[&String],
    offset: usize,
    width: u16,
    selection: Option<&Selection>,
) -> Vec<AnyElement<'static>> {
    let mut elements = Vec::with_capacity(visible_lines.len());
    let line_num_width = 5;
    let content_width = (width as usize).saturating_sub(line_num_width + 3); // "NNNNN │ "

    for (i, line) in visible_lines.iter().enumerate() {
        let line_num = offset + i + 1;
        let log_line_idx = offset + i;

        // Truncate long lines to fit terminal width
        let display_line = if line.len() > content_width {
            format!("{}…", &line[..content_width.saturating_sub(1)])
        } else {
            (*line).clone()
        };

        let line_prefix = format!("{:>5} \u{2502} ", line_num);

        // Check if this line has a selection range
        let sel_range = selection.and_then(|s| s.line_range(log_line_idx, display_line.len()));

        if let Some((start_col, end_col)) = sel_range {
            // Split display_line into before/selected/after segments
            let before: String = display_line.chars().take(start_col).collect();
            let selected: String = display_line
                .chars()
                .skip(start_col)
                .take(end_col - start_col)
                .collect();
            let after: String = display_line.chars().skip(end_col).collect();

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
                            content: format!("{}{}", line_prefix, display_line),
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
