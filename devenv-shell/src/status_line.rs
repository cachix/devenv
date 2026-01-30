//! Status line rendering for shell sessions.
//!
//! Provides a status bar at the bottom of the terminal showing build status,
//! reload readiness, and error messages. Uses iocraft for component-based rendering.

use crossterm::{cursor, execute, terminal};
use iocraft::prelude::*;
use std::collections::HashSet;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

/// Color constants matching devenv-tui styling
const COLOR_ACTIVE: Color = Color::AnsiValue(255); // Bright white
const COLOR_COMPLETED: Color = Color::Rgb {
    r: 112,
    g: 138,
    b: 88,
}; // Sage green
const COLOR_FAILED: Color = Color::AnsiValue(160); // Red
const COLOR_SECONDARY: Color = Color::AnsiValue(242); // Gray

/// Current status state.
#[derive(Debug, Clone, Default)]
pub struct StatusState {
    /// Files that changed (shown during build/reload).
    pub changed_files: Vec<PathBuf>,
    /// Whether a build is in progress (evaluating nix).
    pub building: bool,
    /// Whether a reload is ready (waiting for user).
    pub reload_ready: bool,
    /// Error message if build failed.
    pub error: Option<String>,
    /// Keybind hint for reload.
    keybind: String,
}

impl StatusState {
    /// Create a new empty status state.
    pub fn new() -> Self {
        Self {
            keybind: std::env::var("DEVENV_RELOAD_KEYBIND")
                .unwrap_or_else(|_| "Alt-Ctrl-R".to_string()),
            ..Default::default()
        }
    }

    /// Update state for building status.
    pub fn set_building(&mut self, changed_files: Vec<PathBuf>) {
        self.building = true;
        self.reload_ready = false;
        self.changed_files = changed_files;
        self.error = None;
    }

    /// Update state for reload ready.
    pub fn set_reload_ready(&mut self, changed_files: Vec<PathBuf>, _keybind_hint: &str) {
        self.building = false;
        self.reload_ready = true;
        self.changed_files = changed_files;
        self.error = None;
    }

    /// Update state for build failed.
    pub fn set_build_failed(&mut self, changed_files: Vec<PathBuf>, error: String) {
        self.building = false;
        self.reload_ready = false;
        self.changed_files = changed_files;
        self.error = Some(error);
    }

    /// Set a custom message (for backwards compatibility).
    pub fn set_message(&mut self, _message: String) {
        // No-op for now, state is tracked via building/reload_ready/error
    }

    /// Clear the status.
    pub fn clear(&mut self) {
        self.building = false;
        self.reload_ready = false;
        self.changed_files.clear();
        self.error = None;
    }

    /// Check if there's any status to display.
    pub fn has_status(&self) -> bool {
        self.building || self.reload_ready || self.error.is_some()
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
        .collect();

    if files.is_empty() {
        return String::new();
    }

    if files.len() <= 3 {
        files.join(", ")
    } else {
        format!("{}, +{} more", files[..3].join(", "), files.len() - 3)
    }
}

/// Spinner component for status line.
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SPINNER_INTERVAL_MS: u64 = 80;

#[derive(Default, Props)]
struct SpinnerProps {
    color: Option<Color>,
}

#[component]
fn Spinner(mut hooks: Hooks, props: &SpinnerProps) -> impl Into<AnyElement<'static>> {
    let mut frame = hooks.use_state(|| 0usize);
    let color = props.color.unwrap_or(COLOR_ACTIVE);

    hooks.use_future(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(SPINNER_INTERVAL_MS)).await;
            frame.set((frame.get() + 1) % SPINNER_FRAMES.len());
        }
    });

    element! {
        Text(content: SPINNER_FRAMES[frame.get()], color: color)
    }
}

/// Status line manager using iocraft for rendering.
pub struct StatusLine {
    state: StatusState,
    enabled: bool,
}

impl StatusLine {
    /// Create a new status line.
    pub fn new() -> Self {
        Self {
            state: StatusState::new(),
            enabled: true,
        }
    }

    /// Create with default settings (for backwards compatibility).
    pub fn with_defaults() -> Self {
        Self::new()
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

        // Save cursor position
        execute!(stdout, cursor::SavePosition)?;

        // Move to the last line
        let status_row = rows.saturating_sub(1);
        execute!(stdout, cursor::MoveTo(0, status_row))?;

        // Clear the line
        execute!(stdout, terminal::Clear(terminal::ClearType::CurrentLine))?;

        // Build and render the element
        let mut element = self.build_element(cols);

        // Render to string and write
        let output = element.to_string();
        let truncated: String = output.chars().take(cols as usize).collect();
        write!(stdout, "{}", truncated)?;

        // Restore cursor position
        execute!(stdout, cursor::RestorePosition)?;
        stdout.flush()?;

        Ok(())
    }

    /// Build the status line element.
    fn build_element(&self, width: u16) -> AnyElement<'static> {
        let files_str = format_changed_files(&self.state.changed_files);

        if self.state.building {
            // Building state: spinner + "Reloading" + files
            let text = if files_str.is_empty() {
                "Reloading...".to_string()
            } else {
                format!("Reloading... {}", files_str)
            };

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, background_color: Color::AnsiValue(24)) {
                    View(margin_left: 1, margin_right: 1) {
                        Spinner(color: COLOR_ACTIVE)
                    }
                    View(flex_grow: 1.0, overflow: Overflow::Hidden) {
                        Text(content: text, color: COLOR_ACTIVE)
                    }
                }
            }
            .into_any()
        } else if self.state.reload_ready {
            // Ready state: checkmark + "Ready" + files + keybind hint
            let text = if files_str.is_empty() {
                format!("Ready (press {})", self.state.keybind)
            } else {
                format!("Ready: {} (press {})", files_str, self.state.keybind)
            };

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, background_color: Color::AnsiValue(22)) {
                    View(margin_left: 1, margin_right: 1) {
                        Text(content: "✓", color: COLOR_COMPLETED)
                    }
                    View(flex_grow: 1.0, overflow: Overflow::Hidden) {
                        Text(content: text, color: COLOR_COMPLETED)
                    }
                }
            }
            .into_any()
        } else if let Some(ref error) = self.state.error {
            // Failed state: X + error message
            let text = if files_str.is_empty() {
                format!("Build failed: {}", error)
            } else {
                format!("Build failed ({}): {}", files_str, error)
            };

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, background_color: Color::AnsiValue(52)) {
                    View(margin_left: 1, margin_right: 1) {
                        Text(content: "✗", color: COLOR_FAILED)
                    }
                    View(flex_grow: 1.0, overflow: Overflow::Hidden) {
                        Text(content: text, color: COLOR_FAILED)
                    }
                }
            }
            .into_any()
        } else {
            // Idle state: hint text
            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, background_color: Color::AnsiValue(236)) {
                    View(margin_left: 1, flex_grow: 1.0, overflow: Overflow::Hidden) {
                        Text(content: "devenv shell (--reload)", color: COLOR_SECONDARY)
                    }
                }
            }
            .into_any()
        }
    }
}

impl Default for StatusLine {
    fn default() -> Self {
        Self::new()
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
        assert!(!state.reload_ready);
        assert_eq!(state.changed_files.len(), 1);
    }

    #[test]
    fn test_status_state_reload_ready() {
        let mut state = StatusState::new();
        state.set_reload_ready(vec![PathBuf::from("devenv.nix")], "Alt-Ctrl-R");
        assert!(!state.building);
        assert!(state.reload_ready);
    }

    #[test]
    fn test_status_state_build_failed() {
        let mut state = StatusState::new();
        state.set_build_failed(
            vec![PathBuf::from("devenv.nix")],
            "syntax error".to_string(),
        );
        assert!(!state.building);
        assert!(!state.reload_ready);
        assert!(state.error.is_some());
    }

    #[test]
    fn test_format_changed_files_empty() {
        assert_eq!(format_changed_files(&[]), "");
    }

    #[test]
    fn test_format_changed_files_deduplicates() {
        let files = vec![
            PathBuf::from("/a/devenv.nix"),
            PathBuf::from("/b/devenv.nix"),
            PathBuf::from("/c/other.nix"),
        ];
        let result = format_changed_files(&files);
        assert!(result.contains("devenv.nix"));
        assert!(result.contains("other.nix"));
        // Should only have one devenv.nix
        assert_eq!(result.matches("devenv.nix").count(), 1);
    }

    #[test]
    fn test_format_changed_files_limits() {
        let files = vec![
            PathBuf::from("a.nix"),
            PathBuf::from("b.nix"),
            PathBuf::from("c.nix"),
            PathBuf::from("d.nix"),
        ];
        let result = format_changed_files(&files);
        assert!(result.contains("+1 more"));
    }

    #[test]
    fn test_status_line_state_transitions() {
        let mut sl = StatusLine::new();

        assert!(!sl.state().has_status());

        sl.state_mut().set_building(vec![PathBuf::from("test.nix")]);
        assert!(sl.state().has_status());
        assert!(sl.state().building);

        sl.state_mut()
            .set_reload_ready(vec![PathBuf::from("test.nix")], "Alt-Ctrl-R");
        assert!(sl.state().has_status());
        assert!(sl.state().reload_ready);

        sl.state_mut().clear();
        assert!(!sl.state().has_status());
    }
}
