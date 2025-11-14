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
    let tui_handle = hooks.use_context::<crate::TuiHandle>();
    let model = hooks.use_context::<Arc<Mutex<Model>>>();
    let (terminal_width, _terminal_height) = hooks.use_terminal_size();

    // Use state to trigger re-renders
    let mut tick = hooks.use_state(|| 0u64);

    // Handle keyboard events by sending to event queue
    hooks.use_terminal_events({
        let tui_handle = tui_handle.clone();
        let model = model.clone();
        move |event| {
            if let TerminalEvent::Key(key_event) = event
                && key_event.kind != KeyEventKind::Release
            {
                match key_event.code {
                    KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Ctrl+C needs to set AppState directly for shutdown
                        if let Ok(mut model_guard) = model.lock() {
                            model_guard.app_state = AppState::Shutdown;
                        }
                    }
                    code => {
                        // Send other keyboard events to queue
                        tui_handle.send_ui_event(crate::UiEvent::KeyInput(code));
                    }
                }
            }
        }
    });

    // Trigger re-renders based on tick events
    hooks.use_future({
        let model = model.clone();
        async move {
            let mut counter = 0u64;
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;

                // Check if we should exit
                if let Ok(model_guard) = model.lock() {
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
    let model_clone = Arc::clone(&model);

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
    tui_handle: crate::TuiHandle,
    _shutdown: std::sync::Arc<tokio_shutdown::Shutdown>,
) -> std::io::Result<()> {
    let model = tui_handle.model();

    // Spawn ticker task to send Tick events
    let tui_handle_clone = tui_handle.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            interval.tick().await;
            tui_handle_clone.send_ui_event(crate::UiEvent::Tick);
        }
    });

    // Run the iocraft render loop - tokio-graceful-shutdown will handle cancellation automatically
    let mut tui_element = element! {
        ContextProvider(value: Context::owned(tui_handle)) {
            ContextProvider(value: Context::owned(Arc::clone(&model))) {
                TuiApp
            }
        }
    };

    // No need for select! - tokio-graceful-shutdown handles cancellation
    tui_element.render_loop().await
}
