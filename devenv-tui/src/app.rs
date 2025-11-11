use crate::{
    model::{AppState, Model},
    view::view,
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Main TUI component
#[component]
fn TuiApp(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let model = hooks.use_context::<Arc<Mutex<Model>>>();
    let (terminal_width, _terminal_height) = hooks.use_terminal_size();

    // Use state to trigger re-renders
    let tick = hooks.use_state(|| 0u64);

    // Handle keyboard events directly
    hooks.use_terminal_events({
        let model = model.clone();
        move |event| {
            if let TerminalEvent::Key(key_event) = event
                && key_event.kind != KeyEventKind::Release
                && let Ok(mut model_guard) = model.lock()
            {
                match key_event.code {
                    KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        model_guard.app_state = AppState::Shutdown;
                    }
                    KeyCode::Down => {
                        model_guard.select_next_build();
                    }
                    KeyCode::Up => {
                        model_guard.select_previous_build();
                    }
                    KeyCode::Esc => {
                        model_guard.ui.selected_activity = None;
                    }
                    KeyCode::Char('e') => {
                        model_guard.ui.view_options.show_expanded_logs =
                            !model_guard.ui.view_options.show_expanded_logs;
                    }
                    _ => {}
                }
            }
        }
    });

    // Spinner animation update loop - triggers re-renders via state updates
    hooks.use_future({
        let model = model.clone();
        let mut tick = tick.clone();
        async move {
            let mut counter = 0u64;
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;

                if let Ok(mut model_guard) = model.lock() {
                    // Update spinner animation directly
                    let now = std::time::Instant::now();
                    if now
                        .duration_since(model_guard.ui.last_spinner_update)
                        .as_millis()
                        >= 50
                    {
                        model_guard.ui.spinner_frame = (model_guard.ui.spinner_frame + 1) % 10;
                        model_guard.ui.last_spinner_update = now;
                    }

                    // Check if we should exit
                    if model_guard.app_state == AppState::Shutdown {
                        break;
                    }
                }

                // Trigger re-render by updating state
                counter += 1;
                tick.set(counter);
            }
        }
    });

    // Render the view
    let model_clone = model.clone();

    if let Ok(model_guard) = model_clone.lock() {
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
pub async fn run_app(
    model: Arc<Mutex<Model>>,
    _shutdown: std::sync::Arc<tokio_shutdown::Shutdown>,
) -> std::io::Result<()> {
    // Run the iocraft render loop - tokio-graceful-shutdown will handle cancellation automatically
    let mut tui_element = element! {
        ContextProvider(value: Context::owned(model.clone())) {
            TuiApp
        }
    };

    // No need for select! - tokio-graceful-shutdown handles cancellation
    tui_element
        .render_loop()
        .await
        .map_err(|e| std::io::Error::other(e))
}
