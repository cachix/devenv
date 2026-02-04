//! Status line rendering for shell sessions.
//!
//! Provides a status bar at the bottom of the terminal showing build status,
//! reload readiness, and error messages. Uses iocraft for component-based rendering.
//!
//! Also exports shared UI constants used by both devenv-shell and devenv-tui.

use crossterm::{cursor, execute, terminal};
use iocraft::prelude::*;
use std::collections::HashSet;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Instant;

// ============================================================================
// Shared UI constants - used by both devenv-shell and devenv-tui
// ============================================================================

/// Bright white for active/in-progress items
pub const COLOR_ACTIVE: Color = Color::AnsiValue(255);
/// Dimmer white for nested active items
pub const COLOR_ACTIVE_NESTED: Color = Color::AnsiValue(246);
/// Gray for secondary text (cached, phases, etc.)
pub const COLOR_SECONDARY: Color = Color::AnsiValue(242);
/// Gray for tree lines and elapsed time
pub const COLOR_HIERARCHY: Color = Color::AnsiValue(242);
/// Sage green for success checkmarks
pub const COLOR_COMPLETED: Color = Color::Rgb {
    r: 112,
    g: 138,
    b: 88,
};
/// Red for failed items
pub const COLOR_FAILED: Color = Color::AnsiValue(160);
/// Blue for info indicators
pub const COLOR_INFO: Color = Color::AnsiValue(39);
/// Gold for selected/interactive items
pub const COLOR_INTERACTIVE: Color = Color::AnsiValue(220);

/// Spinner animation frames (braille dots pattern)
pub const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// Spinner animation interval in milliseconds
pub const SPINNER_INTERVAL_MS: u64 = 80;

/// Success checkmark character
pub const CHECKMARK: &str = "✓";
/// Failure X character
pub const XMARK: &str = "✗";

/// Current status state.
#[derive(Debug, Clone)]
pub struct StatusState {
    /// Files that changed (shown during build/reload).
    pub changed_files: Vec<PathBuf>,
    /// Whether a build is in progress (evaluating nix).
    pub building: bool,
    /// Whether a reload is ready (waiting for user).
    pub reload_ready: bool,
    /// Error message if build failed.
    pub error: Option<String>,
    /// Whether file watching is paused.
    pub paused: bool,
    /// Files being watched for changes.
    pub watched_files: Vec<PathBuf>,
    /// When the current build started (for timing).
    build_start: Option<Instant>,
    /// Duration of the last completed build.
    pub build_duration: Option<std::time::Duration>,
}

impl Default for StatusState {
    fn default() -> Self {
        Self {
            changed_files: Vec::new(),
            building: false,
            reload_ready: false,
            error: None,
            paused: false,
            watched_files: Vec::new(),
            build_start: None,
            build_duration: None,
        }
    }
}

impl StatusState {
    /// Create a new empty status state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Update state for building status.
    pub fn set_building(&mut self, changed_files: Vec<PathBuf>) {
        self.building = true;
        self.reload_ready = false;
        self.changed_files = changed_files;
        self.error = None;
        self.build_start = Some(Instant::now());
        self.build_duration = None;
    }

    /// Update state for reload ready.
    pub fn set_reload_ready(&mut self, changed_files: Vec<PathBuf>) {
        // Calculate build duration
        if let Some(start) = self.build_start.take() {
            self.build_duration = Some(start.elapsed());
        }
        self.building = false;
        self.reload_ready = true;
        self.changed_files = changed_files;
        self.error = None;
    }

    /// Update state for build failed.
    pub fn set_build_failed(&mut self, changed_files: Vec<PathBuf>, error: String) {
        // Calculate build duration even for failures
        if let Some(start) = self.build_start.take() {
            self.build_duration = Some(start.elapsed());
        }
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

    /// Set paused state.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Set watched files.
    pub fn set_watched_files(&mut self, files: Vec<PathBuf>) {
        self.watched_files = files;
    }

    /// Check if there's any status to display.
    pub fn has_status(&self) -> bool {
        self.building
            || self.reload_ready
            || self.error.is_some()
            || self.paused
            || !self.watched_files.is_empty()
    }
}

/// Format duration for display, returning (number, unit) for separate coloring.
/// E.g., ("250", "ms"), ("1.2", "s"), ("2m 30", "s")
fn format_duration_parts(duration: std::time::Duration) -> (String, String) {
    let total_secs = duration.as_secs();
    if total_secs < 1 {
        (format!("{}", duration.as_millis()), "ms".to_string())
    } else if total_secs < 60 {
        (format!("{:.1}", duration.as_secs_f64()), "s".to_string())
    } else {
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        (format!("{}m {}", mins, secs), "s".to_string())
    }
}

/// Format duration as a single string (for simple cases).
fn format_duration(duration: std::time::Duration) -> String {
    let (num, unit) = format_duration_parts(duration);
    format!("{}{}", num, unit)
}

/// Format changed files for display, deduplicating and adapting to available space.
fn format_changed_files(changed_files: &[PathBuf], max_len: usize) -> String {
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

    // Try showing progressively fewer files until it fits
    for limit in (1..=3.min(files.len())).rev() {
        let shown: Vec<_> = files.iter().take(limit).cloned().collect();
        let remaining = files.len() - limit;
        let result = if remaining > 0 {
            format!("{} +{}", shown.join(", "), remaining)
        } else {
            shown.join(", ")
        };
        if result.len() <= max_len {
            return result;
        }
    }

    // Last resort: just show count
    if files.len() == 1 {
        let name = &files[0];
        if name.len() <= max_len {
            return name.clone();
        }
        // Truncate single filename
        return format!("{}…", &name[..max_len.saturating_sub(1)]);
    }
    format!("{} files", files.len())
}

/// Status line manager using iocraft for rendering.
pub struct StatusLine {
    state: StatusState,
    enabled: bool,
    /// Current spinner frame index (animated manually since we don't use iocraft runtime)
    spinner_frame: usize,
    /// Last time the spinner frame was updated
    last_spinner_update: Instant,
}

impl StatusLine {
    /// Create a new status line.
    pub fn new() -> Self {
        Self {
            state: StatusState::new(),
            enabled: true,
            spinner_frame: 0,
            last_spinner_update: Instant::now(),
        }
    }

    /// Create with default settings (for backwards compatibility).
    pub fn with_defaults() -> Self {
        Self::new()
    }

    /// Advance spinner animation if enough time has passed.
    fn update_spinner(&mut self) {
        let elapsed = self.last_spinner_update.elapsed().as_millis() as u64;
        if elapsed >= SPINNER_INTERVAL_MS {
            self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
            self.last_spinner_update = Instant::now();
        }
    }

    /// Get the current spinner character.
    fn spinner_char(&self) -> &'static str {
        SPINNER_FRAMES[self.spinner_frame]
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
    pub fn draw(&mut self, stdout: &mut impl Write, cols: u16) -> io::Result<()> {
        if !self.enabled {
            return Ok(());
        }

        // Update spinner animation
        self.update_spinner();

        // Save cursor position
        execute!(stdout, cursor::SavePosition)?;

        // Reset origin mode (DECOM) to ensure absolute cursor positioning,
        // then move to last row using large number (terminals clamp to actual last row)
        write!(stdout, "\x1b[?6l\x1b[999;1H")?;

        // Clear the line
        execute!(stdout, terminal::Clear(terminal::ClearType::CurrentLine))?;

        // Build and render the element with ANSI colors
        let mut element = self.build_element(cols);
        let canvas = element.render(Some(cols as usize));

        // Write only the first line to avoid extra rows
        let mut buffer = Vec::new();
        canvas.write_ansi(&mut buffer)?;
        // Find first newline and truncate there (keep only first line)
        if let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
            buffer.truncate(pos);
        }
        stdout.write_all(&buffer)?;

        // Restore cursor position
        execute!(stdout, cursor::RestorePosition)?;
        stdout.flush()?;

        Ok(())
    }

    /// Build the status line element.
    pub fn build_element(&self, width: u16) -> AnyElement<'static> {
        // Use short keybind notation for narrow terminals
        let use_short = width < 60;
        let keybind = if use_short { "^⌥r" } else { "Ctrl-Alt-R" };
        // Calculate space for files: width - prefix - keybind - margins
        // "devenv shell ready: " = 20, keybind + " reload" = 17/10, margins ~6
        let keybind_len = keybind.len() + 7; // " reload"
        let prefix_len = 23; // "devenv shell reloading: " or similar
        let margins = 6;
        let files_max_len = (width as usize).saturating_sub(prefix_len + keybind_len + margins);
        let files_str = format_changed_files(&self.state.changed_files, files_max_len);

        // Common: file count for right side (number green, "files" gray)
        let watch_count = self.state.watched_files.len();
        let file_count_num = format!("{}", watch_count);

        if self.state.building {
            // Building state: spinner + building message + file count
            let spinner = self.spinner_char().to_string();
            let files_suffix = if files_str.is_empty() {
                String::new()
            } else {
                format!(": {}", files_str)
            };

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding_left: 1, padding_right: 1) {
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                        View(margin_right: 1) {
                            Text(content: spinner, color: COLOR_ACTIVE)
                        }
                        Text(content: "devenv reload ", color: COLOR_SECONDARY)
                        Text(content: "building", weight: Weight::Bold, color: COLOR_ACTIVE)
                        Text(content: files_suffix)
                        Text(content: " | ", color: COLOR_SECONDARY)
                        Text(content: file_count_num.clone(), color: COLOR_COMPLETED)
                        Text(content: " files", color: COLOR_SECONDARY)
                    }
                }
            }
            .into_any()
        } else if self.state.reload_ready {
            // Ready state: "ready in 41ms | devenv.nix changed | 456 files"
            let (duration_num, duration_unit) = self
                .state
                .build_duration
                .map(|d| format_duration_parts(d))
                .unwrap_or_default();
            let has_duration = !duration_num.is_empty();
            let keybind = keybind.to_string();

            // Build the middle section: "| devenv.nix changed" or empty if no files
            let has_changed_files = !files_str.is_empty();

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding_left: 1, padding_right: 1) {
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                        View(margin_right: 1) {
                            Text(content: CHECKMARK, color: COLOR_COMPLETED)
                        }
                        Text(content: "devenv ", color: COLOR_SECONDARY)
                        Text(content: "ready", weight: Weight::Bold, color: COLOR_ACTIVE)
                        #(if has_duration {
                            vec![
                                element!(Text(content: " in ", color: COLOR_SECONDARY)).into_any(),
                                element!(Text(content: duration_num, color: COLOR_COMPLETED)).into_any(),
                                element!(Text(content: format!(" {}", duration_unit), color: COLOR_SECONDARY)).into_any(),
                            ]
                        } else {
                            vec![]
                        })
                        #(if has_changed_files {
                            vec![
                                element!(Text(content: " | ", color: COLOR_SECONDARY)).into_any(),
                                element!(Text(content: files_str.clone(), color: COLOR_COMPLETED)).into_any(),
                                element!(Text(content: " changed", color: COLOR_SECONDARY)).into_any(),
                            ]
                        } else {
                            vec![]
                        })
                        Text(content: " | ", color: COLOR_SECONDARY)
                        Text(content: file_count_num.clone(), color: COLOR_COMPLETED)
                        Text(content: " files", color: COLOR_SECONDARY)
                    }
                    View(flex_direction: FlexDirection::Row, flex_shrink: 0.0, margin_left: 2) {
                        Text(content: keybind, color: COLOR_INTERACTIVE)
                        Text(content: " reload")
                    }
                }
            }
            .into_any()
        } else if let Some(ref error) = self.state.error {
            // Failed state: X + failed message + error + file count
            let duration_str = self
                .state
                .build_duration
                .map(|d| format!(" {}", format_duration(d)))
                .unwrap_or_default();
            let error_str = format!(": {}", error);

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding_left: 1, padding_right: 1) {
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                        View(margin_right: 1) {
                            Text(content: XMARK, color: COLOR_FAILED)
                        }
                        Text(content: "devenv reload ", color: COLOR_SECONDARY)
                        Text(content: "failed", weight: Weight::Bold, color: COLOR_FAILED)
                        Text(content: duration_str, color: COLOR_COMPLETED)
                        Text(content: error_str, color: COLOR_FAILED)
                        Text(content: " | ", color: COLOR_SECONDARY)
                        Text(content: file_count_num.clone(), color: COLOR_COMPLETED)
                        Text(content: " files", color: COLOR_SECONDARY)
                    }
                }
            }
            .into_any()
        } else if self.state.paused {
            // Paused state: pause icon + paused message + file count + keybind
            let pause_keybind = if use_short { "^⌥d" } else { "Ctrl-Alt-D" };

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding_left: 1, padding_right: 1) {
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                        View(margin_right: 1) {
                            Text(content: "⏸", color: COLOR_SECONDARY)
                        }
                        Text(content: "devenv reload ", color: COLOR_SECONDARY)
                        Text(content: "paused", weight: Weight::Bold, color: COLOR_ACTIVE)
                        Text(content: " | ", color: COLOR_SECONDARY)
                        Text(content: file_count_num.clone(), color: COLOR_COMPLETED)
                        Text(content: " files", color: COLOR_SECONDARY)
                    }
                    View(flex_direction: FlexDirection::Row, flex_shrink: 0.0, margin_left: 2) {
                        Text(content: pause_keybind.to_string(), color: COLOR_INTERACTIVE)
                        Text(content: " resume")
                    }
                }
            }
            .into_any()
        } else if !self.state.watched_files.is_empty() {
            // Watching state: eye icon + watching message + file count + keybind
            let pause_keybind = if use_short { "^⌥d" } else { "Ctrl-Alt-D" };

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding_left: 1, padding_right: 1) {
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                        View(margin_right: 2) {
                            Text(content: "👁", color: COLOR_SECONDARY)
                        }
                        Text(content: "devenv ", color: COLOR_SECONDARY)
                        Text(content: "watching", weight: Weight::Bold, color: COLOR_ACTIVE)
                        Text(content: " | ", color: COLOR_SECONDARY)
                        Text(content: file_count_num.clone(), color: COLOR_COMPLETED)
                        Text(content: " files", color: COLOR_SECONDARY)
                    }
                    View(flex_direction: FlexDirection::Row, flex_shrink: 0.0, margin_left: 2) {
                        Text(content: pause_keybind.to_string(), color: COLOR_INTERACTIVE)
                        Text(content: " pause")
                    }
                }
            }
            .into_any()
        } else {
            // Idle state: show nothing
            element! {
                View(width: width as u32, height: 1)
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
        state.set_reload_ready(vec![PathBuf::from("devenv.nix")]);
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
        assert_eq!(format_changed_files(&[], 100), "");
    }

    #[test]
    fn test_format_changed_files_deduplicates() {
        let files = vec![
            PathBuf::from("/a/devenv.nix"),
            PathBuf::from("/b/devenv.nix"),
            PathBuf::from("/c/other.nix"),
        ];
        let result = format_changed_files(&files, 100);
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
        let result = format_changed_files(&files, 100);
        assert!(result.contains("+1"));
    }

    #[test]
    fn test_format_changed_files_shortens() {
        let files = vec![
            PathBuf::from("devenv.nix"),
            PathBuf::from("shell.nix"),
            PathBuf::from("flake.nix"),
        ];
        // With plenty of space, show all
        let wide = format_changed_files(&files, 100);
        assert!(wide.contains("devenv.nix"));
        assert!(wide.contains("shell.nix"));
        assert!(wide.contains("flake.nix"));

        // With limited space, show fewer
        let narrow = format_changed_files(&files, 20);
        assert!(narrow.contains("devenv.nix"));
        assert!(narrow.contains("+2"));
    }

    #[test]
    fn test_status_line_state_transitions() {
        let mut sl = StatusLine::new();

        assert!(!sl.state().has_status());

        sl.state_mut().set_building(vec![PathBuf::from("test.nix")]);
        assert!(sl.state().has_status());
        assert!(sl.state().building);

        sl.state_mut()
            .set_reload_ready(vec![PathBuf::from("test.nix")]);
        assert!(sl.state().has_status());
        assert!(sl.state().reload_ready);

        sl.state_mut().clear();
        assert!(!sl.state().has_status());
    }
}
