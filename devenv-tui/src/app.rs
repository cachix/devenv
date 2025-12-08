use crate::expanded_view::ExpandedLogView;
use crate::{
    model::{Model, ViewMode},
    view::view,
};
use crossterm::{cursor, execute, terminal};
use devenv_activity::ActivityEvent;
use iocraft::prelude::*;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
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

/// Main TUI component
#[component]
fn TuiApp(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let ui_tx = hooks.use_context::<mpsc::Sender<crate::UiEvent>>();
    let model = hooks.use_context::<Arc<Mutex<Model>>>();
    let (terminal_width, terminal_height) = hooks.use_terminal_size();
    let mut should_exit = hooks.use_state(|| false);
    let shutdown = hooks.use_context::<Arc<Shutdown>>();
    let mut system = hooks.use_context_mut::<SystemContext>();

    let send = {
        let ui_tx = ui_tx.clone();
        hooks.use_async_handler(move |event: crate::UiEvent| {
            let ui_tx = ui_tx.clone();
            async move { ui_tx.send(event).await.unwrap() }
        })
    };

    // Track previous terminal size to detect changes
    let mut prev_size = hooks.use_state(crate::TerminalSize::default);
    let current_size = crate::TerminalSize {
        width: terminal_width,
        height: terminal_height,
    };
    if current_size != prev_size.get() {
        prev_size.set(current_size);
        send(crate::UiEvent::Resize(current_size))
    }

    // Trigger periodic re-renders to pick up model changes from background event processing
    // TODO: FIX
    let mut tick = hooks.use_state(|| 0u64);
    hooks.use_future({
        let ui_tx = ui_tx.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
                tick.set(tick + 1);
                ui_tx.send(crate::UiEvent::Tick).await.unwrap();
            }
        }
    });

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
                        // Switch to expanded view if we have a selected activity with logs
                        let mut m = model.lock().unwrap();
                        if let Some(activity_id) = m.ui.selected_activity {
                            m.ui.view_mode = ViewMode::ExpandedLogs {
                                activity_id,
                                scroll_offset: 0,
                            };
                            should_exit.set(true);
                        }
                    }
                    code => send(crate::UiEvent::KeyInput(code)),
                }
            }
        }
    });

    if should_exit.get() || shutdown.is_cancelled() {
        system.exit();
    }

    // Render the view
    if let Ok(model_guard) = model.lock() {
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
    // Create model and UI channel
    let model = Arc::new(Mutex::new(Model::new()));
    let (ui_tx, ui_rx) = mpsc::channel(32);

    // Spawn event processor task (runs throughout, independent of view changes)
    tokio::spawn({
        let model = Arc::clone(&model);
        async move {
            crate::process_events(activity_rx, ui_rx, model).await;
        }
    });

    // Track height to clear when returning from expanded view
    let mut pre_expand_height: u16 = 0;

    loop {
        let ui_tx = ui_tx.clone();
        tokio::select! {
            _ = shutdown.wait_for_shutdown() => {
                break;
            }

            _ =
                run_view(
                    model.clone(),
                    ui_tx,
                    shutdown.clone(),
                    &mut pre_expand_height,
                ) => { }
        }
    }

    // // Clear the TUI content before exiting
    // let height = model.lock().unwrap().calculate_rendered_height();
    // if height > 0 {
    //     let mut stdout = io::stdout();
    //     let _ = execute!(
    //         stdout,
    //         cursor::MoveToPreviousLine(height),
    //         terminal::Clear(terminal::ClearType::FromCursorDown)
    //     );
    // }

    Ok(())
}

async fn run_view(
    model: Arc<Mutex<Model>>,
    ui_tx: mpsc::Sender<crate::UiEvent>,
    shutdown: Arc<Shutdown>,
    pre_expand_height: &mut u16,
) -> std::io::Result<()> {
    let view_mode = { model.lock().unwrap().ui.view_mode.clone() };

    match view_mode {
        ViewMode::Main => {
            if *pre_expand_height > 0 {
                // Clear the old TUI content before restarting
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
                    ContextProvider(value: Context::owned(ui_tx.clone())) {
                        ContextProvider(value: Context::owned(model.clone())) {
                            TuiApp
                        }
                    }
                }
            };

            element.render_loop().ignore_ctrl_c().await
        }
        ViewMode::ExpandedLogs { .. } => {
            // Store the rendered height before switching to expanded view
            *pre_expand_height = model.lock().unwrap().calculate_rendered_height();
            // Run expanded view (fullscreen, uses alternate screen buffer)
            let mut element = element! {
                ContextProvider(value: Context::owned(shutdown.clone())) {
                    ContextProvider(value: Context::owned(ui_tx.clone())) {
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
