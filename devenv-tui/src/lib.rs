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

use tokio::sync::mpsc;

/// Initialize the TUI system and return both the layer and event sender
pub fn init_tui() -> (DevenvTuiLayer, mpsc::UnboundedSender<TuiEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let layer = DevenvTuiLayer::new(tx.clone());

    // Set up a Ctrl-C handler that sends shutdown event, but ignore errors if channel is closed
    let shutdown_tx = tx.clone();
    tokio::spawn(async move {
        // This will catch Ctrl-C if the display doesn't handle it
        if tokio::signal::ctrl_c().await.is_ok() {
            // Only try to send if the sender isn't closed
            if !shutdown_tx.is_closed() {
                let _ = shutdown_tx.send(TuiEvent::Shutdown);
            }
        }
    });

    // Spawn the display thread
    tokio::spawn(async move {
        if let Err(e) = app::run_app(rx).await {
            eprintln!("TEA App error: {}", e);
        }
    });

    (layer, tx)
}

/// Initialize TUI with cancellation token support for proper signal handling integration
pub fn init_tui_with_cancellation(
    cancellation_token: tokio_util::sync::CancellationToken,
) -> (DevenvTuiLayer, mpsc::UnboundedSender<TuiEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let layer = DevenvTuiLayer::new(tx.clone());

    // Set up cancellation handler that sends shutdown event
    let shutdown_tx = tx.clone();
    tokio::spawn(async move {
        cancellation_token.cancelled().await;
        let _ = shutdown_tx.send(TuiEvent::Shutdown);
    });

    // Spawn the display thread
    tokio::spawn(async move {
        if let Err(e) = app::run_app(rx).await {
            eprintln!("TEA App error: {}", e);
        }
    });

    (layer, tx)
}

/// Create a NixLogBridge that can be used to send Nix log events to the TUI
pub fn create_nix_bridge(sender: mpsc::UnboundedSender<TuiEvent>) -> std::sync::Arc<NixLogBridge> {
    std::sync::Arc::new(NixLogBridge::new(sender))
}

/// Cleanup TUI terminal state
pub fn cleanup_tui(sender: &mpsc::UnboundedSender<TuiEvent>) {
    // First, send shutdown event to gracefully close the TUI event loop
    let _ = sender.send(TuiEvent::Shutdown);

    // Give the TUI loop a moment to process the shutdown event
    // TODO: avoid using a sleep. Use a different approach.
    std::thread::sleep(std::time::Duration::from_millis(50));
}
