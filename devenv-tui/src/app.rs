use crate::{
    message::Message,
    model::{AppState, Model},
    update::update,
    view::view,
    TuiEvent,
};
use crossterm::{
    event::{self},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{backend::CrosstermBackend, Terminal, TerminalOptions, Viewport};
use std::{
    io::{self, Stderr},
    time::Duration,
};
use tokio::{
    sync::mpsc,
    time::{interval, MissedTickBehavior},
};

/// The main TUI application following The Elm Architecture
pub struct App {
    /// The application model (state)
    model: Model,

    /// Terminal instance
    terminal: Terminal<CrosstermBackend<Stderr>>,

    /// Channel receiver for TUI events
    event_receiver: mpsc::UnboundedReceiver<TuiEvent>,
}

impl App {
    /// Create a new App instance
    pub fn new(
        event_receiver: mpsc::UnboundedReceiver<TuiEvent>,
        viewport_height: u16,
    ) -> io::Result<Self> {
        // Setup terminal with inline viewport (no alternate screen)
        enable_raw_mode()?;
        let stderr = io::stderr();

        let backend = CrosstermBackend::new(stderr);
        let terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(viewport_height),
            },
        )?;

        let mut model = Model::new();
        model.ui.viewport_height = viewport_height;

        Ok(Self {
            model,
            terminal,
            event_receiver,
        })
    }

    /// Run the main application loop
    pub async fn run(mut self) -> io::Result<()> {
        // Set up ticker for rendering and spinner animation (50ms to match original)
        let mut ticker = interval(Duration::from_millis(50));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        // Main event loop
        loop {
            // Check if we should exit
            if self.model.app_state == AppState::Shutdown {
                break;
            }

            // Always draw the UI every 50ms
            self.terminal.draw(|frame| view(&self.model, frame))?;

            // Handle events with timeout
            tokio::select! {
                // Handle TUI events from the channel
                Some(tui_event) = self.event_receiver.recv() => {
                    let message = Message::TuiEvent(tui_event);
                    if let Some(new_message) = update(&mut self.model, message) {
                        update(&mut self.model, new_message);
                    }
                }

                // Handle terminal events (keyboard, mouse, resize)
                Ok(Ok(true)) = tokio::task::spawn_blocking(|| event::poll(Duration::from_millis(10))) => {
                    if let Ok(event) = event::read() {
                        let message = Message::TerminalEvent(event);
                        if let Some(new_message) = update(&mut self.model, message) {
                            update(&mut self.model, new_message);
                        }
                    }
                }

                // Update spinner animation and trigger redraw
                _ = ticker.tick() => {
                    let message = Message::UpdateSpinner;
                    if let Some(new_message) = update(&mut self.model, message) {
                        update(&mut self.model, new_message);
                    }
                }
            };
        }

        Ok(())
    }
}

impl Drop for App {
    fn drop(&mut self) {
        // Restore terminal on exit
        let _ = disable_raw_mode();
        let _ = self.terminal.show_cursor();
        // Clear the inline viewport area
        let _ = execute!(
            io::stderr(),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::CurrentLine),
            crossterm::cursor::Show
        );
    }
}

/// Create and run a new TUI application with TEA architecture
pub async fn run_app(event_receiver: mpsc::UnboundedReceiver<TuiEvent>) -> io::Result<()> {
    // Default viewport height
    let viewport_height = 20;

    // Create and run the app
    let app = App::new(event_receiver, viewport_height)?;
    app.run().await
}
