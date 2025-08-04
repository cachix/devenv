use crate::{
    message::{key_event_to_message, Message},
    model::{AppState, Model},
    update::update,
    view::view,
    TuiEvent,
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// Shared application state
pub struct SharedAppState {
    model: Model,
    event_receiver: mpsc::UnboundedReceiver<TuiEvent>,
}

/// Main TUI component
#[component]
fn TuiApp(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let app_state = hooks.use_context::<Arc<Mutex<SharedAppState>>>();
    let (terminal_width, _terminal_height) = hooks.use_terminal_size();

    // Force re-render on state changes
    let mut render_tick = hooks.use_state(|| 0);

    // Handle keyboard events
    hooks.use_terminal_events({
        let app_state = app_state.clone();
        move |event| {
            if let TerminalEvent::Key(key_event) = event {
                if key_event.kind != KeyEventKind::Release {
                    if let Ok(mut state) = app_state.lock() {
                        let message = key_event_to_message(key_event);
                        if let Some(new_message) = update(&mut state.model, message) {
                            update(&mut state.model, new_message);
                        }
                    }
                }
            }
        }
    });

    // Update loop for spinner animation and processing TUI events
    hooks.use_future({
        let app_state = app_state.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;

                if let Ok(mut state) = app_state.lock() {
                    // Process any pending TUI events
                    while let Ok(tui_event) = state.event_receiver.try_recv() {
                        let message = Message::TuiEvent(tui_event);
                        if let Some(new_message) = update(&mut state.model, message) {
                            update(&mut state.model, new_message);
                        }
                    }

                    // Update spinner animation
                    let message = Message::UpdateSpinner;
                    if let Some(new_message) = update(&mut state.model, message) {
                        update(&mut state.model, new_message);
                    }

                    // Check if we should exit
                    if state.model.app_state == AppState::Shutdown {
                        break;
                    }
                }

                // Force re-render
                render_tick += 1;
            }
        }
    });

    // Render the view
    let app_state_clone = app_state.clone();
    let result = if let Ok(state) = app_state_clone.lock() {
        element! {
            View(width: terminal_width) {
                #(vec![view(&state.model).into()])
            }
        }
    } else {
        element!(View(width: terminal_width))
    };
    result
}

/// Create and run the TUI application
pub async fn run_app(event_receiver: mpsc::UnboundedReceiver<TuiEvent>) -> std::io::Result<()> {
    let mut model = Model::new();
    model.ui.viewport_height = 20;

    let app_state = Arc::new(Mutex::new(SharedAppState {
        model,
        event_receiver,
    }));

    // Run the iocraft render loop
    element! {
        ContextProvider(value: Context::owned(app_state)) {
            TuiApp
        }
    }
    .render_loop()
    .await
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}
