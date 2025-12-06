use crate::{UiSender, model::Model, view::view};
use crossterm::{cursor, execute, terminal};
use devenv_activity::ActivityEvent;
use iocraft::prelude::*;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_shutdown::Shutdown;

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
    let ui_sender = hooks.use_context::<UiSender>();
    let model = hooks.use_context::<Arc<Mutex<Model>>>();
    let shutdown = hooks.use_context::<Arc<Shutdown>>();
    let (terminal_width, terminal_height) = hooks.use_terminal_size();

    // Track previous terminal size to detect changes
    let mut prev_size = hooks.use_state(crate::TerminalSize::default);
    let current_size = crate::TerminalSize {
        width: terminal_width,
        height: terminal_height,
    };
    if current_size != prev_size.get() {
        prev_size.set(current_size);
        ui_sender.send(crate::UiEvent::Resize(current_size));
    }

    // Trigger periodic re-renders to pick up model changes from background event processing
    let mut tick = hooks.use_state(|| 0u64);
    hooks.use_future({
        let ui_sender = ui_sender.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
                tick.set(tick + 1);
                ui_sender.send(crate::UiEvent::Tick);
            }
        }
    });

    // Handle keyboard events
    hooks.use_terminal_events({
        let ui_sender = ui_sender.clone();
        let shutdown = shutdown.clone();
        move |event| {
            if let TerminalEvent::Key(key_event) = event
                && key_event.kind != KeyEventKind::Release
            {
                match key_event.code {
                    KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        shutdown.shutdown();
                    }
                    code => {
                        ui_sender.send(crate::UiEvent::KeyInput(code));
                    }
                }
            }
        }
    });

    // Check if shutdown was requested and exit cleanly
    if shutdown.is_cancelled() {
        hooks.use_context_mut::<SystemContext>().exit();
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
pub async fn run_app(
    activity_rx: mpsc::UnboundedReceiver<ActivityEvent>,
    shutdown: Arc<Shutdown>,
) -> std::io::Result<()> {
    // Create model and UI channel
    let model = Arc::new(Mutex::new(Model::new()));
    let (ui_sender, ui_rx) = crate::create_ui_channel();

    // Spawn event processor task
    let model_clone = Arc::clone(&model);
    tokio::spawn(async move {
        crate::process_events(activity_rx, ui_rx, model_clone).await;
    });

    // Run the iocraft render loop
    let mut tui_element = element! {
        ContextProvider(value: Context::owned(shutdown)) {
            ContextProvider(value: Context::owned(ui_sender)) {
                ContextProvider(value: Context::owned(Arc::clone(&model))) {
                    TuiApp
                }
            }
        }
    };

    tui_element.render_loop().await
}
