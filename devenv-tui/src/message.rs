use crate::{OperationId, TuiEvent};
use iocraft::prelude::*;

/// Messages that drive state changes in the application
/// Following The Elm Architecture pattern
#[derive(Debug, Clone)]
pub enum Message {
    /// A TUI event was received (from the existing event system)
    TuiEvent(TuiEvent),

    /// Keyboard event
    KeyEvent(KeyEvent),

    /// Update spinner animation
    UpdateSpinner,

    /// Select an operation
    SelectOperation(OperationId),

    /// Clear selection
    ClearSelection,

    /// Toggle detailed view
    ToggleDetails,

    /// Toggle expanded logs view
    ToggleExpandedLogs,

    /// Select next activity
    SelectNextActivity,

    /// Select previous activity
    SelectPreviousActivity,

    /// Request shutdown
    RequestShutdown,

    /// No operation (used when update doesn't produce a new message)
    None,
}

impl From<TuiEvent> for Message {
    fn from(event: TuiEvent) -> Self {
        Message::TuiEvent(event)
    }
}

impl From<KeyEvent> for Message {
    fn from(event: KeyEvent) -> Self {
        Message::KeyEvent(event)
    }
}

/// Convert keyboard events to messages
pub fn key_event_to_message(key: KeyEvent) -> Message {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Message::RequestShutdown
        }
        KeyCode::Down => Message::SelectNextActivity,
        KeyCode::Up => Message::SelectPreviousActivity,
        KeyCode::Esc => Message::ClearSelection,
        KeyCode::Char('e') => Message::ToggleExpandedLogs,
        _ => Message::None,
    }
}
