use crate::{ActivityModel, TerminalSize, UiState};
use iocraft::KeyCode;

/// Low-volume UI control events
///
/// These events represent user interactions and UI state changes.
/// They are processed immediately with priority over activity events for responsiveness.
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// Keyboard input from user
    KeyInput(KeyCode),

    /// Terminal size changed
    Resize(TerminalSize),
}

impl UiEvent {
    /// Process this UI event by applying it to UI state
    ///
    /// UI events are processed immediately with priority for responsiveness.
    /// Activity model is only read (for getting selectable activities).
    pub fn apply(self, activity_model: &ActivityModel, ui_state: &mut UiState) {
        match self {
            UiEvent::KeyInput(key_code) => {
                use KeyCode::*;
                match key_code {
                    Down => {
                        let selectable = activity_model.get_selectable_activity_ids();
                        ui_state.select_next_activity(&selectable);
                    }
                    Up => {
                        let selectable = activity_model.get_selectable_activity_ids();
                        ui_state.select_previous_activity(&selectable);
                    }
                    Esc => {
                        ui_state.selected_activity = None;
                    }
                    // Note: 'e' for expand is handled directly in TuiApp to trigger view switch
                    _ => {}
                }
            }

            UiEvent::Resize(size) => {
                ui_state.set_terminal_size(size.width, size.height);
            }
        }
    }
}
