use devenv_tui::{Model, view::view};
use iocraft::prelude::*;

/// Render the TUI view to a string with a fixed width for reproducible snapshots.
pub fn render_to_string(model: &Model, width: usize) -> String {
    let mut element = view(model).into();
    element.render(Some(width)).to_string()
}

/// Render the TUI view to a string with default width (80 columns).
pub fn render_to_string_default(model: &Model) -> String {
    render_to_string(model, 80)
}
