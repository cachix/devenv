pub mod display;
pub mod events;
pub mod state;
pub mod tracing_layer;

pub use display::{DefaultDisplay, FallbackDisplay};
pub use events::*;
pub use state::TuiState;
pub use tracing_layer::DevenvTuiLayer;

use std::sync::Arc;
use tokio::sync::mpsc;

/// Display mode for the TUI
#[derive(Debug, Clone, Copy)]
pub enum DisplayMode {
    /// Full TUI interface
    Tui,
    /// Simple console output (fallback)
    Console,
}

/// Initialize the TUI system with the specified display mode
pub fn init_tui(mode: DisplayMode) -> (DevenvTuiLayer, Arc<TuiState>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let state = Arc::new(TuiState::new());
    let layer = DevenvTuiLayer::new(tx, state.clone());

    // Spawn the display thread based on mode
    let display_state = state.clone();
    tokio::spawn(async move {
        match mode {
            DisplayMode::Tui => {
                let mut display = DefaultDisplay::new(rx, display_state);
                display.run().await;
            }
            DisplayMode::Console => {
                let mut display = FallbackDisplay::new(rx, display_state);
                display.run().await;
            }
        }
    });

    (layer, state)
}
