//! Status line rendering for shell sessions.
//!
//! Provides a status bar at the bottom of the terminal showing build status,
//! reload readiness, and error messages.

use crossterm::{
    cursor, execute,
    style::{Color, Print, SetBackgroundColor, SetForegroundColor},
    terminal::{self, ClearType},
};
use std::collections::HashSet;
use std::io::{self, Write};
use std::path::PathBuf;

/// Current status state.
#[derive(Debug, Clone, Default)]
pub struct StatusState {
    /// Current status message.
    pub message: Option<String>,
    /// Files that changed (shown during build/reload).
    pub changed_files: Vec<PathBuf>,
    /// Whether a build is in progress (evaluating nix).
    pub building: bool,
    /// Whether a reload is in progress (applying env to shell).
    pub reloading: bool,
}

impl StatusState {
    /// Create a new empty status state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Update state for building status.
    pub fn set_building(&mut self, changed_files: Vec<PathBuf>) {
        self.building = true;
        self.reloading = false;
        self.changed_files = changed_files;
        self.message = None;
    }

    /// Update state for reload ready.
    pub fn set_reload_ready(&mut self, changed_files: Vec<PathBuf>, keybind_hint: &str) {
        self.building = false;
        self.reloading = false;
        let files_str = format_changed_files(&changed_files);
        self.message = Some(format!("Ready: {} (press {})", files_str, keybind_hint));
    }

    /// Update state for build failed.
    pub fn set_build_failed(&mut self, changed_files: Vec<PathBuf>, error: String) {
        self.building = false;
        self.reloading = false;
        let files_str = format_changed_files(&changed_files);
        self.message = Some(format!("Build failed ({}): {}", files_str, error));
    }

    /// Set a custom message.
    pub fn set_message(&mut self, message: String) {
        self.message = Some(message);
    }
}

/// Format changed files for display, deduplicating and limiting to 3.
fn format_changed_files(changed_files: &[PathBuf]) -> String {
    let mut seen = HashSet::new();
    let files: Vec<_> = changed_files
        .iter()
        .filter_map(|p| {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if seen.insert(name.clone()) {
                Some(name)
            } else {
                None
            }
        })
        .take(3)
        .collect();
    files.join(", ")
}

/// Trait for customizing status line rendering.
pub trait StatusRenderer: Send {
    /// Render the status line content.
    /// Returns (text, background_color).
    fn render(&self, state: &StatusState, width: u16) -> (String, Color);
}

/// Default status renderer with devenv styling.
pub struct DefaultStatusRenderer {
    #[allow(dead_code)]
    reload_keybind: String,
}

impl DefaultStatusRenderer {
    /// Create a new default renderer.
    pub fn new() -> Self {
        let reload_keybind =
            std::env::var("DEVENV_RELOAD_KEYBIND").unwrap_or_else(|_| "Alt-Ctrl-R".to_string());
        Self { reload_keybind }
    }

    /// Create with a specific keybind hint.
    pub fn with_keybind(keybind: String) -> Self {
        Self {
            reload_keybind: keybind,
        }
    }
}

impl Default for DefaultStatusRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusRenderer for DefaultStatusRenderer {
    fn render(&self, state: &StatusState, _width: u16) -> (String, Color) {
        if state.building {
            // Building environment (Nix evaluation in progress)
            let files = format_changed_files(&state.changed_files);
            (format!(" Building... [{}]", files), Color::Blue)
        } else if state.reloading {
            // Applying environment to shell
            let files = format_changed_files(&state.changed_files);
            (format!(" Reloading... [{}]", files), Color::Yellow)
        } else if let Some(ref msg) = state.message {
            // Check if message indicates success or failure
            let color = if msg.starts_with("Ready:") {
                Color::Green
            } else if msg.contains("failed") {
                Color::Red
            } else {
                Color::DarkGrey
            };
            (format!(" {}", msg), color)
        } else {
            (" devenv shell (--reload)".to_string(), Color::DarkGrey)
        }
    }
}

/// Status line manager.
pub struct StatusLine {
    renderer: Box<dyn StatusRenderer>,
    state: StatusState,
    enabled: bool,
}

impl StatusLine {
    /// Create a new status line with a custom renderer.
    pub fn new(renderer: Box<dyn StatusRenderer>) -> Self {
        Self {
            renderer,
            state: StatusState::new(),
            enabled: true,
        }
    }

    /// Create with default renderer.
    pub fn with_defaults() -> Self {
        Self::new(Box::new(DefaultStatusRenderer::new()))
    }

    /// Enable or disable the status line.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if the status line is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get mutable access to the state.
    pub fn state_mut(&mut self) -> &mut StatusState {
        &mut self.state
    }

    /// Get access to the state.
    pub fn state(&self) -> &StatusState {
        &self.state
    }

    /// Draw the status line at the bottom of the terminal.
    pub fn draw(&self, stdout: &mut impl Write, rows: u16, cols: u16) -> io::Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let (status_text, bg_color) = self.renderer.render(&self.state, cols);

        // Save cursor position
        execute!(stdout, cursor::SavePosition)?;

        // Move to the last line
        let status_row = rows.saturating_sub(1);
        execute!(stdout, cursor::MoveTo(0, status_row))?;

        // Clear the line
        execute!(stdout, terminal::Clear(ClearType::CurrentLine))?;

        // Draw status with colors
        execute!(
            stdout,
            SetBackgroundColor(bg_color),
            SetForegroundColor(Color::White),
            Print(format!("{:<width$}", status_text, width = cols as usize)),
            SetBackgroundColor(Color::Reset),
            SetForegroundColor(Color::Reset)
        )?;

        // Restore cursor position
        execute!(stdout, cursor::RestorePosition)?;
        stdout.flush()?;

        Ok(())
    }
}

impl Default for StatusLine {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_state_building() {
        let mut state = StatusState::new();
        state.set_building(vec![PathBuf::from("devenv.nix")]);
        assert!(state.building);
        assert!(!state.reloading);
        assert_eq!(state.changed_files.len(), 1);
    }

    #[test]
    fn test_status_state_reload_ready() {
        let mut state = StatusState::new();
        state.set_reload_ready(vec![PathBuf::from("devenv.nix")], "Alt-Ctrl-R");
        assert!(!state.building);
        assert!(state.message.is_some());
        assert!(state.message.as_ref().unwrap().contains("Ready:"));
    }

    #[test]
    fn test_format_changed_files_deduplicates() {
        let files = vec![
            PathBuf::from("/a/devenv.nix"),
            PathBuf::from("/b/devenv.nix"),
            PathBuf::from("/c/other.nix"),
        ];
        let result = format_changed_files(&files);
        // Should deduplicate devenv.nix
        assert!(result.contains("devenv.nix"));
        assert!(result.contains("other.nix"));
    }

    #[test]
    fn test_default_renderer() {
        let renderer = DefaultStatusRenderer::new();
        let state = StatusState::new();
        let (text, color) = renderer.render(&state, 80);
        assert!(text.contains("devenv shell"));
        assert_eq!(color, Color::DarkGrey);
    }

    #[test]
    fn test_default_renderer_building() {
        let renderer = DefaultStatusRenderer::new();
        let mut state = StatusState::new();
        state.set_building(vec![PathBuf::from("test.nix")]);
        let (text, color) = renderer.render(&state, 80);
        assert!(text.contains("Building"));
        assert_eq!(color, Color::Blue);
    }
}
