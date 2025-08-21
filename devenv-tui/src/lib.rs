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

/// Handle for TUI system with proper shutdown tracking
pub struct TuiHandle {
    pub layer: DevenvTuiLayer,
    pub sender: mpsc::UnboundedSender<TuiEvent>,
}

impl TuiHandle {
    /// Get a clone of the event sender
    pub fn sender(&self) -> mpsc::UnboundedSender<TuiEvent> {
        self.sender.clone()
    }
}

/// Initialize the TUI system and return handle + future to spawn
///
/// Returns (handle, future) where the future should be spawned with shutdown.spawn_task()
pub fn init_tui() -> (
    TuiHandle,
    impl std::future::Future<Output = std::io::Result<()>> + Send + 'static,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let layer = DevenvTuiLayer::new(tx.clone());

    // Return the app future directly - tokio-graceful handles cancellation
    let future = app::run_app(rx);

    let handle = TuiHandle { layer, sender: tx };

    (handle, future)
}

/// Create a NixLogBridge that can be used to send Nix log events to the TUI
pub fn create_nix_bridge(sender: mpsc::UnboundedSender<TuiEvent>) -> std::sync::Arc<NixLogBridge> {
    std::sync::Arc::new(NixLogBridge::new(sender))
}
