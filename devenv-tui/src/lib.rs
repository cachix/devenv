pub mod display;
pub mod events;
pub mod state;
pub mod tracing_layer;

pub use display::{DefaultDisplay, FallbackDisplay, RatatuiDisplay};
pub use events::*;
pub use state::TuiState;
pub use tracing_layer::DevenvTuiLayer;

use std::sync::Arc;
use tokio::sync::mpsc;

/// Display mode for the TUI
#[derive(Debug, Clone, Copy)]
pub enum DisplayMode {
    /// Enhanced TUI interface with inline viewport (recommended)
    Ratatui,
    /// Basic TUI interface (legacy)
    Tui,
    /// Simple console output (fallback)
    Console,
}

/// Initialize the TUI system with the specified display mode
pub fn init_tui(mode: DisplayMode) -> (DevenvTuiLayer, Arc<TuiState>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let state = Arc::new(TuiState::new());
    let layer = DevenvTuiLayer::new(tx, state.clone());

    // Set up signal handler for graceful TUI cleanup on Ctrl+C
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigint =
                signal(SignalKind::interrupt()).expect("Failed to register SIGINT handler");
            let mut sigterm =
                signal(SignalKind::terminate()).expect("Failed to register SIGTERM handler");

            tokio::select! {
                _ = sigint.recv() => {
                    cleanup_before_exec();
                    std::process::exit(130); // Standard exit code for SIGINT
                }
                _ = sigterm.recv() => {
                    cleanup_before_exec();
                    std::process::exit(143); // Standard exit code for SIGTERM
                }
            }
        }
    });

    // Spawn the display thread based on mode
    let display_state = state.clone();
    tokio::spawn(async move {
        match mode {
            DisplayMode::Ratatui => {
                match RatatuiDisplay::new(rx, display_state.clone()) {
                    Ok(mut display) => {
                        if let Err(e) = display.run().await {
                            eprintln!("RatatuiDisplay error: {}", e);
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "Failed to create RatatuiDisplay: {}, falling back to console mode",
                            e
                        );
                        // Create a new channel for fallback since rx was moved
                        let (_fallback_tx, fallback_rx) = mpsc::unbounded_channel::<TuiEvent>();
                        let mut display = FallbackDisplay::new(fallback_rx, display_state);
                        // Note: This fallback loses events that were sent to the original channel
                        // In practice, this should rarely happen since RatatuiDisplay creation
                        // typically only fails due to terminal initialization issues
                        display.run().await;
                    }
                }
            }
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

/// Force cleanup of TUI to prevent terminal corruption before exec
pub fn cleanup_before_exec() {
    use std::io::Write;

    // Force terminal restoration to prevent corruption when process is replaced by exec
    ratatui::restore();

    // Ensure cursor is visible after cleanup
    if crossterm::execute!(std::io::stderr(), crossterm::cursor::Show).is_err() {
        // Fallback if crossterm fails - try basic ANSI escape
        let _ = std::io::stderr().write_all(b"\x1b[?25h");
        let _ = std::io::stderr().flush();
    }
}
