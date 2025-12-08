use crate::{
    expanded_view::ExpandedLogView,
    model::{Model, ViewMode},
    view::view,
};
use crossterm::{cursor, execute, terminal};
use devenv_activity::ActivityEvent;
use iocraft::prelude::*;
use std::io::{self, Write};
use std::sync::{Arc, RwLock};
use tokio::sync::{mpsc, Notify};
use tokio_shutdown::Shutdown;
use tracing::debug;

/// Restore terminal to normal state.
/// Call this after the TUI has exited to ensure the terminal is usable.
pub fn restore_terminal() {
    let mut stdout = io::stdout();

    // Disable raw mode if it was enabled
    let _ = terminal::disable_raw_mode();

    // Show cursor (TUI may have hidden it)
    let _ = execute!(stdout, cursor::Show);

    // Ensure output is flushed
    let _ = stdout.flush();
}

/// Main TUI component (inline mode)
#[component]
fn TuiApp(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let model = hooks.use_context::<Arc<RwLock<Model>>>();
    let notify = hooks.use_context::<Arc<Notify>>();
    let (terminal_width, terminal_height) = hooks.use_terminal_size();
    let mut should_exit = hooks.use_state(|| false);
    let shutdown = hooks.use_context::<Arc<Shutdown>>();
    let mut system = hooks.use_context_mut::<SystemContext>();

    // Redraw when notified of model changes
    let mut redraw = hooks.use_state(|| 0u64);
    hooks.use_future({
        let notify = notify.clone();
        async move {
            loop {
                notify.notified().await;
                redraw.set(redraw.get().wrapping_add(1));
            }
        }
    });

    // Track terminal size changes
    let mut prev_size = hooks.use_state(crate::TerminalSize::default);
    let current_size = crate::TerminalSize {
        width: terminal_width,
        height: terminal_height,
    };
    if current_size != prev_size.get() {
        prev_size.set(current_size);
        if let Ok(mut m) = model.write() {
            m.set_terminal_size(current_size.width, current_size.height);
        }
    }

    // Handle keyboard events
    hooks.use_terminal_events({
        let model = model.clone();
        let shutdown = shutdown.clone();

        move |event| {
            if let TerminalEvent::Key(key_event) = event
                && key_event.kind != KeyEventKind::Release
            {
                debug!("Key event: {:?}", key_event);
                match key_event.code {
                    KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        shutdown.shutdown();
                    }
                    KeyCode::Char('e') => {
                        if let Ok(mut m) = model.write() {
                            if let Some(activity_id) = m.ui.selected_activity {
                                m.ui.view_mode = ViewMode::ExpandedLogs {
                                    activity_id,
                                    scroll_offset: 0,
                                };
                                should_exit.set(true);
                            }
                        }
                    }
                    KeyCode::Down => {
                        if let Ok(mut m) = model.write() {
                            m.select_next_activity();
                        }
                    }
                    KeyCode::Up => {
                        if let Ok(mut m) = model.write() {
                            m.select_previous_activity();
                        }
                    }
                    KeyCode::Esc => {
                        if let Ok(mut m) = model.write() {
                            m.ui.selected_activity = None;
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    if should_exit.get() || shutdown.is_cancelled() {
        system.exit();
    }

    // Render the view
    if let Ok(model_guard) = model.read() {
        element! {
            View(width: terminal_width) {
                #(vec![view(&model_guard).into()])
            }
        }
    } else {
        element!(View(width: terminal_width))
    }
}

/// Create and run the TUI application
///
/// Takes ownership of the activity event receiver and sets up all TUI internals.
/// The application switches between main view (non-fullscreen) and expanded view
/// (fullscreen with alternate screen buffer) based on user input.
pub async fn run_app(
    activity_rx: mpsc::Receiver<ActivityEvent>,
    shutdown: Arc<Shutdown>,
) -> std::io::Result<()> {
    let model = Arc::new(RwLock::new(Model::new()));
    let notify = Arc::new(Notify::new());

    // Spawn event processor
    tokio::spawn({
        let model = model.clone();
        let notify = notify.clone();
        let mut activity_rx = activity_rx;
        async move {
            while let Some(event) = activity_rx.recv().await {
                if let Ok(mut m) = model.write() {
                    m.apply_activity_event(event);
                }
                notify.notify_waiters();
            }
        }
    });

    // Track height to clear when returning from expanded view
    let mut pre_expand_height: u16 = 0;

    loop {
        tokio::select! {
            _ = shutdown.wait_for_shutdown() => {
                break;
            }

            _ = run_view(
                model.clone(),
                notify.clone(),
                shutdown.clone(),
                &mut pre_expand_height,
            ) => { }
        }
    }

    Ok(())
}

async fn run_view(
    model: Arc<RwLock<Model>>,
    notify: Arc<Notify>,
    shutdown: Arc<Shutdown>,
    pre_expand_height: &mut u16,
) -> std::io::Result<()> {
    let view_mode = { model.read().unwrap().ui.view_mode.clone() };

    match view_mode {
        ViewMode::Main => {
            if *pre_expand_height > 0 {
                let mut stdout = io::stdout();
                let _ = execute!(
                    stdout,
                    cursor::MoveToPreviousLine(*pre_expand_height),
                    terminal::Clear(terminal::ClearType::FromCursorDown)
                );
                *pre_expand_height = 0;
            }

            let mut element = element! {
                ContextProvider(value: Context::owned(shutdown.clone())) {
                    ContextProvider(value: Context::owned(notify.clone())) {
                        ContextProvider(value: Context::owned(model.clone())) {
                            TuiApp
                        }
                    }
                }
            };

            element.render_loop().ignore_ctrl_c().await
        }
        ViewMode::ExpandedLogs { .. } => {
            *pre_expand_height = model.read().unwrap().calculate_rendered_height();

            let mut element = element! {
                ContextProvider(value: Context::owned(shutdown.clone())) {
                    ContextProvider(value: Context::owned(notify.clone())) {
                        ContextProvider(value: Context::owned(model.clone())) {
                            ExpandedLogView
                        }
                    }
                }
            };

            element.fullscreen().ignore_ctrl_c().await
        }
    }
}
