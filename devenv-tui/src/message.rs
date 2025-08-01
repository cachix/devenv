use crate::{OperationId, TuiEvent};
use crossterm::event::{Event, KeyCode, KeyEvent};

/// Messages that drive state changes in the application
/// Following The Elm Architecture pattern
#[derive(Debug, Clone)]
pub enum Message {
    /// A TUI event was received (from the existing event system)
    TuiEvent(TuiEvent),

    /// Terminal event (keyboard, mouse, resize)
    TerminalEvent(Event),

    /// Update spinner animation
    UpdateSpinner,

    /// Select an operation
    SelectOperation(OperationId),

    /// Clear selection
    ClearSelection,

    /// Toggle detailed view
    ToggleDetails,

    /// Select next activity
    SelectNextActivity,

    /// Select previous activity
    SelectPreviousActivity,

    /// Select specific activity by index
    SelectActivity(usize),

    /// Scroll logs up
    ScrollLogsUp(usize),

    /// Scroll logs down  
    ScrollLogsDown(usize),

    /// Reset log scroll
    ResetLogScroll,

    /// Resize viewport
    ResizeViewport(u16),

    /// Adjust viewport height to fit content
    AdjustViewportHeight,

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

impl From<Event> for Message {
    fn from(event: Event) -> Self {
        Message::TerminalEvent(event)
    }
}

/// Convert keyboard events to messages
pub fn key_event_to_message(key: KeyEvent) -> Message {
    match key.code {
        KeyCode::Char('c')
            if key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL) =>
        {
            Message::RequestShutdown
        }
        KeyCode::Down => Message::SelectNextActivity,
        KeyCode::Up => Message::SelectPreviousActivity,
        KeyCode::Esc => Message::ClearSelection,
        _ => Message::None,
    }
}
