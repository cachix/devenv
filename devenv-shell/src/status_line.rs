//! Status line rendering for shell sessions.
//!
//! Provides a status bar at the bottom of the terminal showing build status,
//! reload readiness, and error messages. Uses iocraft for component-based rendering.
//!
//! Also exports shared UI constants used by both devenv-shell and devenv-tui.

use crossterm::{cursor, queue, style::ResetColor, terminal::Clear, terminal::ClearType};
use iocraft::prelude::*;
use std::collections::HashSet;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Instant;

// ============================================================================
// Shared UI constants - used by both devenv-shell and devenv-tui
// ============================================================================

/// Default foreground for active/in-progress items (adapts to terminal theme)
pub const COLOR_ACTIVE: Color = Color::Reset;
/// Dimmer text for nested active items
pub const COLOR_ACTIVE_NESTED: Color = Color::DarkGrey;
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

/// Keybind labels (short, long) for status line actions
const KEYBIND_ERROR: (&str, &str) = ("^⌥e", "Ctrl-Alt-E");
const KEYBIND_PAUSE: (&str, &str) = ("^⌥d", "Ctrl-Alt-D");

/// Current status state.
#[derive(Debug, Clone, Default)]
pub struct StatusState {
    /// Files that changed (shown during build/reload).
    pub changed_files: Vec<PathBuf>,
    /// Whether a build is in progress (evaluating nix).
    pub building: bool,
    /// Whether a reload is ready (auto-applies at next prompt).
    pub reload_ready: bool,
    /// Whether the environment was just reloaded.
    pub reloaded: bool,
    /// Error message if build failed.
    pub error: Option<String>,
    /// Whether the error details are expanded (toggled by keybind).
    pub show_error: bool,
    /// Whether file watching is paused.
    pub paused: bool,
    /// Number of files being watched for changes.
    pub watched_file_count: usize,
    /// When the current build started (for timing).
    build_start: Option<Instant>,
    /// Duration of the last completed build.
    pub build_duration: Option<std::time::Duration>,
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
        self.reloaded = false;
        self.changed_files = changed_files;
        self.error = None;
        self.show_error = false;
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

    /// Update state after reload was applied.
    pub fn set_reloaded(&mut self) {
        self.building = false;
        self.reload_ready = false;
        self.reloaded = true;
        self.changed_files.clear();
        self.error = None;
        self.show_error = false;
        // keep build_duration and watched_file_count
    }

    /// Clear the status.
    pub fn clear(&mut self) {
        self.building = false;
        self.reload_ready = false;
        self.reloaded = false;
        self.changed_files.clear();
        self.error = None;
        self.show_error = false;
    }

    /// Set paused state.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Set watched file count.
    pub fn set_watched_file_count(&mut self, count: usize) {
        self.watched_file_count = count;
    }

    /// Check if there's any status to display.
    pub fn has_status(&self) -> bool {
        self.building
            || self.reload_ready
            || self.reloaded
            || self.error.is_some()
            || self.paused
            || self.watched_file_count > 0
    }
}

/// Format duration for display, returning (number, unit) for separate coloring.
/// E.g., ("250", "ms"), ("1.2", "s"), ("2m 30", "s")
fn format_duration_parts(duration: std::time::Duration) -> (String, String) {
    if cfg!(feature = "deterministic-tui") {
        return ("[TIME]".to_string(), String::new());
    }
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

/// Select short or long keybind label based on terminal width.
fn keybind_label(keybind: (&'static str, &'static str), use_short: bool) -> &'static str {
    if use_short { keybind.0 } else { keybind.1 }
}

/// Build "in N.Ns" duration elements, or empty if no duration recorded.
fn duration_elements(state: &StatusState) -> Vec<AnyElement<'static>> {
    let Some((num, unit)) = state.build_duration.map(format_duration_parts) else {
        return vec![];
    };
    vec![
        element!(Text(content: " in ", color: COLOR_SECONDARY)).into_any(),
        element!(Text(content: num, color: COLOR_COMPLETED)).into_any(),
        element!(Text(content: unit, color: COLOR_SECONDARY)).into_any(),
    ]
}

/// Build "| watching N files" elements, or empty if no watched files.
fn watching_elements(count: usize) -> Vec<AnyElement<'static>> {
    if count == 0 {
        return vec![];
    }
    vec![
        element!(Text(content: " | watching ", color: COLOR_SECONDARY)).into_any(),
        element!(Text(content: count.to_string(), color: COLOR_COMPLETED)).into_any(),
        element!(Text(content: " files", color: COLOR_SECONDARY)).into_any(),
    ]
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

    /// Advance spinner animation if enough time has passed.
    fn update_spinner(&mut self) {
        if cfg!(feature = "deterministic-tui") {
            return;
        }
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

    /// Draw the status line at the given row of the terminal.
    ///
    /// The caller is responsible for repositioning the cursor after this call.
    pub fn draw(&mut self, stdout: &mut impl Write, cols: u16, total_rows: u16) -> io::Result<()> {
        if !self.enabled {
            return Ok(());
        }

        // Update spinner animation
        self.update_spinner();

        // Build the status line content
        let mut element = self.build_element(cols);
        let canvas = element.render(Some(cols as usize));
        let mut content = Vec::new();
        canvas.write_ansi(&mut content)?;
        // Truncate at first newline to keep only first line
        if let Some(pos) = content.iter().position(|&b| b == b'\n') {
            content.truncate(pos);
        }

        // Move to the last row, clear it, write content
        queue!(
            stdout,
            cursor::MoveTo(0, total_rows - 1),
            Clear(ClearType::CurrentLine)
        )?;
        stdout.write_all(&content)?;
        queue!(stdout, ResetColor)?;

        Ok(())
    }

    /// Build the status line element.
    pub fn build_element(&self, width: u16) -> AnyElement<'static> {
        // Use short keybind notation for narrow terminals
        let use_short = width < 60;

        if self.state.building {
            // Building state: spinner + elapsed time + changed files
            let spinner = self.spinner_char().to_string();
            let elapsed = self
                .state
                .build_start
                .map(|s| format_duration_parts(s.elapsed()));

            // Changed files inline
            let files_max_len = (width as usize).saturating_sub(40);
            let files_str = format_changed_files(&self.state.changed_files, files_max_len);
            let has_changed_files = !files_str.is_empty();

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding_left: 1, padding_right: 1) {
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                        View(margin_right: 1) {
                            Text(content: spinner, color: COLOR_ACTIVE)
                        }
                        Text(content: "devenv ", color: COLOR_SECONDARY)
                        Text(content: "building", weight: Weight::Bold, color: COLOR_ACTIVE)
                        #(if let Some((num, unit)) = elapsed {
                            vec![
                                element!(Text(content: " for ", color: COLOR_SECONDARY)).into_any(),
                                element!(Text(content: num, color: COLOR_COMPLETED)).into_any(),
                                element!(Text(content: unit, color: COLOR_SECONDARY)).into_any(),
                            ]
                        } else {
                            vec![]
                        })
                        #(if has_changed_files {
                            vec![
                                element!(Text(content: ", changed ", color: COLOR_SECONDARY)).into_any(),
                                element!(Text(content: files_str, color: COLOR_COMPLETED)).into_any(),
                            ]
                        } else {
                            vec![]
                        })
                    }
                }
            }
            .into_any()
        } else if self.state.reload_ready {
            // Ready state (auto-reloads at next prompt)
            let duration = duration_elements(&self.state);
            let watching = watching_elements(self.state.watched_file_count);

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding_left: 1, padding_right: 1) {
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                        View(margin_right: 1) {
                            Text(content: CHECKMARK, color: COLOR_COMPLETED)
                        }
                        Text(content: "devenv ", color: COLOR_SECONDARY)
                        Text(content: "ready", weight: Weight::Bold, color: COLOR_ACTIVE)
                        #(duration)
                        #(watching)
                    }
                }
            }
            .into_any()
        } else if self.state.reloaded {
            // Reloaded state (environment was applied)
            let duration = duration_elements(&self.state);
            let watching = watching_elements(self.state.watched_file_count);

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding_left: 1, padding_right: 1) {
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                        View(margin_right: 1) {
                            Text(content: CHECKMARK, color: COLOR_COMPLETED)
                        }
                        Text(content: "devenv ", color: COLOR_SECONDARY)
                        Text(content: "reloaded", weight: Weight::Bold, color: COLOR_COMPLETED)
                        #(duration)
                        #(watching)
                    }
                }
            }
            .into_any()
        } else if self.state.error.is_some() {
            // Failed state
            let duration = duration_elements(&self.state);
            let watching = watching_elements(self.state.watched_file_count);
            let keybind = keybind_label(KEYBIND_ERROR, use_short);
            let error_action = if self.state.show_error {
                " hide error"
            } else {
                " show error"
            };

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding_left: 1, padding_right: 1) {
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                        View(margin_right: 1) {
                            Text(content: XMARK, color: COLOR_FAILED)
                        }
                        Text(content: "devenv ", color: COLOR_SECONDARY)
                        Text(content: "failed", weight: Weight::Bold, color: COLOR_FAILED)
                        #(duration)
                        #(watching)
                    }
                    View(flex_direction: FlexDirection::Row, flex_shrink: 0.0, margin_left: 2) {
                        Text(content: keybind, color: COLOR_INTERACTIVE)
                        Text(content: error_action)
                    }
                }
            }
            .into_any()
        } else if self.state.paused {
            // Paused state
            let keybind = keybind_label(KEYBIND_PAUSE, use_short);

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding_left: 1, padding_right: 1) {
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                        View(margin_right: 2) {
                            Text(content: "⏸", color: COLOR_SECONDARY)
                        }
                        Text(content: "devenv ", color: COLOR_SECONDARY)
                        Text(content: "paused", weight: Weight::Bold, color: COLOR_ACTIVE)
                    }
                    View(flex_direction: FlexDirection::Row, flex_shrink: 0.0, margin_left: 2) {
                        Text(content: keybind, color: COLOR_INTERACTIVE)
                        Text(content: " resume")
                    }
                }
            }
            .into_any()
        } else if self.state.watched_file_count > 0 {
            // Watching state
            let keybind = keybind_label(KEYBIND_PAUSE, use_short);
            let count_str = self.state.watched_file_count.to_string();

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding_left: 1, padding_right: 1) {
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0, min_width: 0, overflow: Overflow::Hidden) {
                        View(margin_right: 2) {
                            Text(content: "👁", color: COLOR_SECONDARY)
                        }
                        Text(content: "devenv ", color: COLOR_SECONDARY)
                        Text(content: "watching ", weight: Weight::Bold, color: COLOR_ACTIVE)
                        Text(content: count_str, color: COLOR_COMPLETED)
                        Text(content: " files", color: COLOR_SECONDARY)
                    }
                    View(flex_direction: FlexDirection::Row, flex_shrink: 0.0, margin_left: 2) {
                        Text(content: keybind, color: COLOR_INTERACTIVE)
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
