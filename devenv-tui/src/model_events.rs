use crate::TerminalSize;
use iocraft::KeyCode;

/// Low-volume UI control events
///
/// These events represent user interactions and UI state changes.
/// They are processed immediately with priority over activity events for responsiveness.
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// Keyboard input from user
    KeyInput(KeyCode),

    /// Animation tick for spinner updates
    Tick,

    /// Terminal size changed
    Resize(TerminalSize),
}

impl UiEvent {
    /// Process this UI event by applying it to the model
    ///
    /// UI events are processed immediately with priority for responsiveness.
    pub fn apply(self, model: &mut crate::Model) {
        match self {
            UiEvent::KeyInput(key_code) => {
                use KeyCode::*;
                match key_code {
                    Down => {
                        model.select_next_activity();
                    }
                    Up => {
                        model.select_previous_activity();
                    }
                    Esc => {
                        model.ui.selected_activity = None;
                    }
                    // Note: 'e' for expand is handled directly in TuiApp to trigger view switch
                    _ => {}
                }
            }

            UiEvent::Tick => {
                // Update spinner animation
                let now = std::time::Instant::now();
                if now.duration_since(model.ui.last_spinner_update).as_millis() >= 50 {
                    model.ui.spinner_frame = (model.ui.spinner_frame + 1) % 10;
                    model.ui.last_spinner_update = now;
                }
            }

            UiEvent::Resize(size) => {
                model.set_terminal_size(size.width, size.height);
            }
        }
    }
}
