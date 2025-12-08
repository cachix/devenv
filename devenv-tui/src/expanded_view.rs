//! Expanded log view component.
//!
//! This component displays build logs in a fullscreen view using the alternate screen buffer.
//! It provides scrollable access to all log lines for a selected activity.

use crate::model::ViewMode;
use crate::Model;
use iocraft::prelude::*;
use iocraft::{FullscreenMouseEvent, MouseEventKind};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_shutdown::Shutdown;

/// Fullscreen component for viewing expanded logs.
///
/// This component runs in fullscreen mode (alternate screen buffer) to avoid
/// affecting terminal scrollback. It provides vim-like navigation for scrolling
/// through log content.
#[component]
pub fn ExpandedLogView(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let model = hooks.use_context::<Arc<Mutex<Model>>>();
    let shutdown = hooks.use_context::<Arc<Shutdown>>();
    let (width, height) = hooks.use_terminal_size();

    // Trigger periodic re-renders to pick up new logs
    let mut tick = hooks.use_state(|| 0u64);
    hooks.use_future(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            tick.set(tick.get() + 1);
        }
    });

    // Extract view state from model
    let view_state = {
        let model_guard = model.lock().unwrap();
        extract_view_state(&model_guard)
    };

    let Some(state) = view_state else {
        // Not in expanded mode, exit immediately
        hooks.use_context_mut::<SystemContext>().exit();
        return element!(View).into_any();
    };

    // Handle keyboard and mouse events
    hooks.use_terminal_events({
        let model = model.clone();
        let shutdown = shutdown.clone();
        let total_lines = state.total_lines;
        let viewport_height = calculate_viewport_height(height);
        move |event| {
            handle_terminal_event(
                event,
                &model,
                &shutdown,
                total_lines,
                viewport_height,
            );
        }
    });

    // Check exit conditions (only view mode - shutdown is handled by select! in run_expanded_view)
    {
        let model_guard = model.lock().unwrap();
        if !matches!(model_guard.ui.view_mode, ViewMode::ExpandedLogs { .. }) {
            hooks.use_context_mut::<SystemContext>().exit();
            return element!(View).into_any();
        }
    }

    render_expanded_view(&state, width, height)
}

/// State extracted from the model for rendering
struct ExpandedViewState {
    activity_name: String,
    scroll_offset: usize,
    logs: VecDeque<String>,
    total_lines: usize,
}

/// Extract the view state from the model
fn extract_view_state(model: &Model) -> Option<ExpandedViewState> {
    if let ViewMode::ExpandedLogs {
        activity_id,
        scroll_offset,
    } = &model.ui.view_mode
    {
        let activity_name = model
            .activities
            .get(activity_id)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| format!("Activity {}", activity_id));

        let logs = model
            .get_build_logs(*activity_id)
            .cloned()
            .unwrap_or_default();

        let total_lines = logs.len();

        Some(ExpandedViewState {
            activity_name,
            scroll_offset: *scroll_offset,
            logs,
            total_lines,
        })
    } else {
        None
    }
}

/// Calculate the viewport height (total height minus header and footer)
fn calculate_viewport_height(terminal_height: u16) -> usize {
    (terminal_height as usize).saturating_sub(2) // header + footer
}

/// Handle terminal input (keyboard and mouse) for the expanded view
fn handle_terminal_event(
    event: TerminalEvent,
    model: &Arc<Mutex<Model>>,
    shutdown: &Arc<Shutdown>,
    total_lines: usize,
    viewport_height: usize,
) {
    match event {
        TerminalEvent::Key(key_event) => {
            if key_event.kind == KeyEventKind::Release {
                return;
            }
            handle_key_event(key_event, model, shutdown, total_lines, viewport_height);
        }
        TerminalEvent::FullscreenMouse(mouse_event) => {
            handle_mouse_event(mouse_event, model, total_lines, viewport_height);
        }
        TerminalEvent::Resize(_, _) | _ => {}
    }
}

/// Handle keyboard input
fn handle_key_event(
    key_event: KeyEvent,
    model: &Arc<Mutex<Model>>,
    shutdown: &Arc<Shutdown>,
    total_lines: usize,
    viewport_height: usize,
) {
    let mut model_guard = model.lock().unwrap();

    match key_event.code {
        // Exit expanded view
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('e') => {
            model_guard.ui.view_mode = ViewMode::Main;
        }

        // Scroll down one line
        KeyCode::Down | KeyCode::Char('j') => {
            if let ViewMode::ExpandedLogs { scroll_offset, .. } = &mut model_guard.ui.view_mode {
                let max_offset = total_lines.saturating_sub(viewport_height);
                *scroll_offset = (*scroll_offset + 1).min(max_offset);
            }
        }

        // Scroll up one line
        KeyCode::Up | KeyCode::Char('k') => {
            if let ViewMode::ExpandedLogs { scroll_offset, .. } = &mut model_guard.ui.view_mode {
                *scroll_offset = scroll_offset.saturating_sub(1);
            }
        }

        // Page down
        KeyCode::PageDown | KeyCode::Char(' ') => {
            if let ViewMode::ExpandedLogs { scroll_offset, .. } = &mut model_guard.ui.view_mode {
                let max_offset = total_lines.saturating_sub(viewport_height);
                *scroll_offset = (*scroll_offset + viewport_height).min(max_offset);
            }
        }

        // Page up
        KeyCode::PageUp => {
            if let ViewMode::ExpandedLogs { scroll_offset, .. } = &mut model_guard.ui.view_mode {
                *scroll_offset = scroll_offset.saturating_sub(viewport_height);
            }
        }

        // Go to top
        KeyCode::Home | KeyCode::Char('g') => {
            if let ViewMode::ExpandedLogs { scroll_offset, .. } = &mut model_guard.ui.view_mode {
                *scroll_offset = 0;
            }
        }

        // Go to bottom
        KeyCode::End | KeyCode::Char('G') => {
            if let ViewMode::ExpandedLogs { scroll_offset, .. } = &mut model_guard.ui.view_mode {
                *scroll_offset = total_lines.saturating_sub(viewport_height);
            }
        }

        // Ctrl+C to shutdown
        KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
            drop(model_guard);
            shutdown.shutdown();
        }

        _ => {}
    }
}

/// Handle mouse input (scroll wheel)
fn handle_mouse_event(
    mouse_event: FullscreenMouseEvent,
    model: &Arc<Mutex<Model>>,
    total_lines: usize,
    viewport_height: usize,
) {
    let mut model_guard = model.lock().unwrap();
    let scroll_lines = 3; // Lines to scroll per wheel tick

    match mouse_event.kind {
        MouseEventKind::ScrollDown => {
            if let ViewMode::ExpandedLogs { scroll_offset, .. } = &mut model_guard.ui.view_mode {
                let max_offset = total_lines.saturating_sub(viewport_height);
                *scroll_offset = (*scroll_offset + scroll_lines).min(max_offset);
            }
        }
        MouseEventKind::ScrollUp => {
            if let ViewMode::ExpandedLogs { scroll_offset, .. } = &mut model_guard.ui.view_mode {
                *scroll_offset = scroll_offset.saturating_sub(scroll_lines);
            }
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
