pub mod display;
pub mod events;
pub mod graph_display;
pub mod nix_bridge;
pub mod state;
pub mod tracing_layer;

pub use display::{DefaultDisplay, FallbackDisplay, RatatuiDisplay};
pub use events::*;
pub use nix_bridge::NixLogBridge;
pub use state::TuiState;
pub use tracing_layer::DevenvTuiLayer;

use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

// Global sender to allow cleanup to send shutdown event
static GLOBAL_SENDER: Mutex<Option<mpsc::UnboundedSender<TuiEvent>>> = Mutex::new(None);

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
    let layer = DevenvTuiLayer::new(tx.clone(), state.clone());

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
                        eprintln!("Failed to create RatatuiDisplay: {}", e);
                        // For terminal initialization errors, try to clean up and fall back
                        let _ = crossterm::terminal::disable_raw_mode();
                        let _ = crossterm::execute!(
                            std::io::stderr(),
                            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
                            crossterm::cursor::Show
                        );

                        eprintln!("Falling back to console mode");
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

/// Create a NixLogBridge that can be used to send Nix log events to the TUI
pub fn create_nix_bridge() -> Option<Arc<NixLogBridge>> {
    if let Ok(sender) = GLOBAL_SENDER.lock() {
        if let Some(tx) = sender.as_ref() {
            return Some(Arc::new(NixLogBridge::new(tx.clone())));
        }
    }
    None
}

/// Cleanup TUI terminal state
pub fn cleanup_tui() {
    use ratatui::{backend::CrosstermBackend, Terminal};
    use std::io::Write;

    // First, send shutdown event to gracefully close the TUI event loop
    if let Ok(sender) = GLOBAL_SENDER.lock() {
        if let Some(tx) = sender.as_ref() {
            let _ = tx.send(TuiEvent::Shutdown);
        }
    }

    // Give the TUI loop a moment to process the shutdown event
    // TODO: avoid using a sleep. Use a different approach.
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Move cursor to the bottom of the active region
    if let Ok(terminal) = Terminal::new(CrosstermBackend::new(std::io::stderr())) {
        if let Ok(size) = terminal.size() {
            let _ = crossterm::execute!(
                std::io::stderr(),
                crossterm::cursor::MoveTo(0, size.height - 1)
            );
        }
    }

    // Disable raw mode first
    let _ = crossterm::terminal::disable_raw_mode();

    ratatui::restore();

    // Insert a new line to push content down and show cursor
    if crossterm::execute!(
        std::io::stderr(),
        crossterm::cursor::MoveToNextLine(1),
        crossterm::cursor::Show
    )
    .is_err()
    {
        // Fallback if crossterm fails - try basic ANSI escapes
        let _ = std::io::stderr().write_all(b"\x1b[E\x1b[?25h"); // Move to next line + show cursor
        let _ = std::io::stderr().flush();
    }
}
