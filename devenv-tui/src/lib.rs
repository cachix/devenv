pub mod events;
pub mod nix_bridge;
pub mod tracing_layer;

// The Elm Architecture modules
pub mod app;
pub mod components;
pub mod message;
pub mod model;
pub mod update;
pub mod view;

pub use events::*;
pub use nix_bridge::NixLogBridge;
pub use tracing_layer::DevenvTuiLayer;

use std::sync::Mutex;
use tokio::sync::mpsc;

// Global sender to allow cleanup to send shutdown event
static GLOBAL_SENDER: Mutex<Option<mpsc::UnboundedSender<TuiEvent>>> = Mutex::new(None);

/// Initialize the TUI system
pub fn init_tui() -> DevenvTuiLayer {
    let (tx, rx) = mpsc::unbounded_channel();
    let layer = DevenvTuiLayer::new(tx.clone());

    // Store the sender globally for cleanup
    if let Ok(mut sender) = GLOBAL_SENDER.lock() {
        *sender = Some(tx.clone());
    }

    // Set up a global Ctrl-C handler that sends shutdown event
    let shutdown_tx = tx.clone();
    tokio::spawn(async move {
        // This will catch Ctrl-C if the display doesn't handle it
        tokio::signal::ctrl_c().await.ok();
        let _ = shutdown_tx.send(TuiEvent::Shutdown);
    });

    // Spawn the display thread
    tokio::spawn(async move {
        if let Err(e) = app::run_app(rx).await {
            eprintln!("TEA App error: {}", e);
        }
    });

    layer
}

/// Create a NixLogBridge that can be used to send Nix log events to the TUI
pub fn create_nix_bridge() -> Option<std::sync::Arc<NixLogBridge>> {
    if let Ok(sender) = GLOBAL_SENDER.lock() {
        if let Some(tx) = sender.as_ref() {
            return Some(std::sync::Arc::new(NixLogBridge::new(tx.clone())));
        }
    }
    None
}

/// Get the global event sender for sending TUI events
pub fn get_event_sender() -> Option<mpsc::UnboundedSender<TuiEvent>> {
    if let Ok(sender) = GLOBAL_SENDER.lock() {
        sender.clone()
    } else {
        None
    }
}

/// Cleanup TUI terminal state
pub fn cleanup_tui() {
    // First, send shutdown event to gracefully close the TUI event loop
    if let Ok(sender) = GLOBAL_SENDER.lock() {
        if let Some(tx) = sender.as_ref() {
            let _ = tx.send(TuiEvent::Shutdown);
        }
    }

    // Give the TUI loop a moment to process the shutdown event
    // TODO: avoid using a sleep. Use a different approach.
    std::thread::sleep(std::time::Duration::from_millis(50));
}
