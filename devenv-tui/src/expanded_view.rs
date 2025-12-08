//! Expanded log view component.
//!
//! This component displays build logs in a fullscreen view using the alternate screen buffer.
//! It provides scrollable access to all log lines for a selected activity.
//! Scroll offset is managed as component-local state for immediate responsiveness.

use crate::model::{ActivityModel, UiState, ViewMode};
use crate::TuiConfig;
use iocraft::prelude::*;
use iocraft::{FullscreenMouseEvent, MouseEventKind};
use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use tokio::sync::Notify;
use tokio_shutdown::Shutdown;

/// Fullscreen component for viewing expanded logs.
///
/// This component runs in fullscreen mode (alternate screen buffer) to avoid
/// affecting terminal scrollback. It provides vim-like navigation for scrolling
/// through log content.
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

    // Handle keyboard and mouse events - NO MODEL LOCK, only local state updates
    hooks.use_terminal_events({
        let ui_state = ui_state.clone();
        let shutdown = shutdown.clone();
        move |event| {
            match event {
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
                    );
                }
                TerminalEvent::FullscreenMouse(mouse_event) => {
                    handle_mouse_event(
                        mouse_event,
                        &mut scroll_offset,
                        total_lines,
                        viewport_height,
                    );
                }
                TerminalEvent::Resize(_, _) | _ => {}
            }
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

    render_expanded_view(&state, width, height)
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

/// Handle keyboard input - updates local scroll state, no model lock needed
fn handle_key_event(
    key_event: KeyEvent,
    ui_state: &Arc<RwLock<UiState>>,
    shutdown: &Arc<Shutdown>,
    scroll_offset: &mut State<usize>,
    total_lines: usize,
    viewport_height: usize,
) {
    let max_offset = total_lines.saturating_sub(viewport_height);

    match key_event.code {
        // Exit expanded view
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('e') => {
            if let Ok(mut ui) = ui_state.write() {
                ui.view_mode = ViewMode::Main;
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
            shutdown.shutdown();
        }

        _ => {}
    }
}

/// Handle mouse input (scroll wheel) - updates local scroll state, no model lock needed
fn handle_mouse_event(
    mouse_event: FullscreenMouseEvent,
    scroll_offset: &mut State<usize>,
    total_lines: usize,
    viewport_height: usize,
) {
    let scroll_lines = 3; // Lines to scroll per wheel tick
    let max_offset = total_lines.saturating_sub(viewport_height);

    match mouse_event.kind {
        MouseEventKind::ScrollDown => {
            scroll_offset.set((scroll_offset.get() + scroll_lines).min(max_offset));
        }
        MouseEventKind::ScrollUp => {
            scroll_offset.set(scroll_offset.get().saturating_sub(scroll_lines));
        }
        _ => {}
    }
}

/// Render the expanded view UI
fn render_expanded_view(state: &ExpandedViewState, width: u16, height: u16) -> AnyElement<'static> {
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
    let line_elements = build_line_elements(&visible_lines, clamped_offset, width);

    // Build empty line padding
    let padding_elements = build_padding_elements(visible_lines.len(), viewport_height);

    // Combine all content elements
    let mut content_elements = line_elements;
    content_elements.extend(padding_elements);

    // Build progress indicator
    let progress = build_progress_indicator(clamped_offset, viewport_height, state.total_lines);

    element! {
        View(
            flex_direction: FlexDirection::Column,
            height: height as u32,
            width: width as u32
        ) {
            // Header
            View(height: 1, padding_left: 1, padding_right: 1) {
                Text(
                    content: format!("─── {} ───", state.activity_name),
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
                    content: format!(
                        "{} │ j/k:line  PgUp/PgDn:page  g/G:top/bottom  q:back",
                        progress
                    ),
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
) -> Vec<AnyElement<'static>> {
    let mut elements = Vec::with_capacity(visible_lines.len());
    let line_num_width = 5;
    let content_width = (width as usize).saturating_sub(line_num_width + 3); // "NNNNN │ "

    for (i, line) in visible_lines.iter().enumerate() {
        let line_num = offset + i + 1;

        // Truncate long lines to fit terminal width
        let display_line = if line.len() > content_width {
            format!("{}…", &line[..content_width.saturating_sub(1)])
        } else {
            (*line).clone()
        };

        elements.push(
            element! {
                View(height: 1) {
                    Text(
                        content: format!("{:>5} │ {}", line_num, display_line),
                        color: Color::AnsiValue(250)
                    )
                }
            }
            .into_any(),
        );
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
