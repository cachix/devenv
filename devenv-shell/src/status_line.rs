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

use crate::keybindings::{ShellAction, ShellKeybindings};

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
/// Yellow for transient/in-progress process states (starting, waiting, draining)
pub const COLOR_TRANSIENT: Color = Color::AnsiValue(214);

/// Spinner animation frames (braille dots pattern)
pub const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// Spinner animation interval in milliseconds
pub const SPINNER_INTERVAL_MS: u64 = 80;

/// Process status dot glyphs. Shape encodes lifecycle so state reads without
/// relying on color (color only reinforces): inert ring, half = transitioning,
/// fisheye = alive, full = ready.
/// Inert / not started.
pub const DOT_INERT: &str = "◌";
/// Idle ring (waiting on deps, or cleanly stopped).
pub const DOT_RING: &str = "○";
/// Transitioning (starting / restarting / draining).
pub const DOT_HALF: &str = "◐";
/// Running without a readiness probe (alive, unverified).
pub const DOT_RUNNING: &str = "◉";
/// Ready (readiness probe passed).
pub const DOT_READY: &str = "●";
/// Interval between pulse (dim/bright) toggles for transient dots, in ms.
pub const PULSE_INTERVAL_MS: u64 = 500;

/// Success checkmark character
pub const CHECKMARK: &str = "✓";
/// Failure X character
pub const XMARK: &str = "✗";

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
    /// When the reloaded state was set (for auto-clearing after timeout).
    pub reloaded_at: Option<Instant>,
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
        self.reloaded_at = None;
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
        self.reloaded_at = Some(Instant::now());
        self.changed_files.clear();
        self.error = None;
        self.show_error = false;
        // keep build_duration and watched_file_count
    }

    /// Duration until the reloaded state should auto-clear.
    pub fn reloaded_remaining(&self) -> Option<std::time::Duration> {
        const RELOADED_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
        self.reloaded_at.and_then(|at| {
            let elapsed = at.elapsed();
            if elapsed >= RELOADED_TIMEOUT {
                None
            } else {
                Some(RELOADED_TIMEOUT - elapsed)
            }
        })
    }

    /// Clear the reloaded state (called when timeout expires).
    pub fn clear_reloaded(&mut self) {
        self.reloaded = false;
        self.reloaded_at = None;
    }

    /// Clear the status.
    pub fn clear(&mut self) {
        self.building = false;
        self.reload_ready = false;
        self.reloaded = false;
        self.reloaded_at = None;
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
    keybindings: ShellKeybindings,
    enabled: bool,
    /// Current spinner frame index (animated manually since we don't use iocraft runtime)
    spinner_frame: usize,
    /// Last time the spinner frame was updated
    last_spinner_update: Instant,
    /// ANSI for the last rendered status-line state. Most calls to `draw` are
    /// caused by PTY output, not a status change, so retaining this avoids
    /// rebuilding the iocraft element tree and canvas for those calls.
    cached_content: Vec<u8>,
    cached_state: Option<CachedRenderState>,
    cached_width: u16,
    cached_spinner_frame: Option<usize>,
}

/// The subset of [`StatusState`] that can affect the rendered status line.
///
/// This deliberately omits state that is hidden by a higher-priority status
/// (for example, a watched-file count while paused). That keeps a cache hit
/// cheap without changing the bytes written to the terminal. Changed paths are
/// retained rather than hashed, so cache validity is exact rather than
/// probabilistic.
#[derive(Clone, Debug, PartialEq, Eq)]
enum CachedRenderState {
    Building {
        changed_files: Vec<PathBuf>,
        elapsed: Option<RenderedElapsed>,
    },
    ReloadReady {
        build_duration: Option<std::time::Duration>,
        watched_file_count: usize,
    },
    Reloaded {
        build_duration: Option<std::time::Duration>,
        watched_file_count: usize,
    },
    Failed {
        build_duration: Option<std::time::Duration>,
        watched_file_count: usize,
        show_error: bool,
    },
    Paused,
    Watching {
        watched_file_count: usize,
    },
    Idle,
}

/// A non-allocating, conservative cache key for a displayed build duration.
///
/// The narrow guard around a decimal rounding boundary uses the full duration;
/// everywhere else a 50 ms bucket is sufficient. This may occasionally redraw
/// before the text changes, but it can never retain stale duration text.
#[derive(Clone, Debug, PartialEq, Eq)]
enum RenderedElapsed {
    Deterministic,
    Milliseconds(u128),
    TenthsOfSecond(u128),
    TenthsBoundary(u128),
    WholeSeconds(u64),
}

impl CachedRenderState {
    fn matches(&self, state: &StatusState, build_elapsed: Option<std::time::Duration>) -> bool {
        match self {
            Self::Building {
                changed_files,
                elapsed,
            } => {
                state.building
                    && changed_files == &state.changed_files
                    && *elapsed == rendered_elapsed(build_elapsed)
            }
            Self::ReloadReady {
                build_duration,
                watched_file_count,
            } => {
                !state.building
                    && state.reload_ready
                    && *build_duration == state.build_duration
                    && *watched_file_count == state.watched_file_count
            }
            Self::Reloaded {
                build_duration,
                watched_file_count,
            } => {
                !state.building
                    && !state.reload_ready
                    && state.reloaded
                    && *build_duration == state.build_duration
                    && *watched_file_count == state.watched_file_count
            }
            Self::Failed {
                build_duration,
                watched_file_count,
                show_error,
            } => {
                !state.building
                    && !state.reload_ready
                    && !state.reloaded
                    && state.error.is_some()
                    && *build_duration == state.build_duration
                    && *watched_file_count == state.watched_file_count
                    && *show_error == state.show_error
            }
            Self::Paused => {
                !state.building
                    && !state.reload_ready
                    && !state.reloaded
                    && state.error.is_none()
                    && state.paused
            }
            Self::Watching { watched_file_count } => {
                !state.building
                    && !state.reload_ready
                    && !state.reloaded
                    && state.error.is_none()
                    && !state.paused
                    && *watched_file_count == state.watched_file_count
                    && state.watched_file_count > 0
            }
            Self::Idle => !state.has_status(),
        }
    }

    fn from_state(state: &StatusState, build_elapsed: Option<std::time::Duration>) -> Self {
        if state.building {
            Self::Building {
                changed_files: state.changed_files.clone(),
                elapsed: rendered_elapsed(build_elapsed),
            }
        } else if state.reload_ready {
            Self::ReloadReady {
                build_duration: state.build_duration,
                watched_file_count: state.watched_file_count,
            }
        } else if state.reloaded {
            Self::Reloaded {
                build_duration: state.build_duration,
                watched_file_count: state.watched_file_count,
            }
        } else if state.error.is_some() {
            Self::Failed {
                build_duration: state.build_duration,
                watched_file_count: state.watched_file_count,
                show_error: state.show_error,
            }
        } else if state.paused {
            Self::Paused
        } else if state.watched_file_count > 0 {
            Self::Watching {
                watched_file_count: state.watched_file_count,
            }
        } else {
            Self::Idle
        }
    }
}

/// Return a non-allocating key matching [`format_duration_parts`].
fn rendered_elapsed(elapsed: Option<std::time::Duration>) -> Option<RenderedElapsed> {
    if cfg!(feature = "deterministic-tui") {
        elapsed.map(|_| RenderedElapsed::Deterministic)
    } else {
        elapsed.map(|elapsed| {
            let total_secs = elapsed.as_secs();
            if total_secs < 1 {
                RenderedElapsed::Milliseconds(elapsed.as_millis())
            } else if total_secs < 60 {
                let millis = elapsed.as_millis();
                // A `f64` formatter's half-tenth boundary lies at 50 ms. The
                // guard also covers its representational edge, while the rest
                // of the range gets 50 ms cache buckets without allocations.
                if (45..=55).contains(&(millis % 100)) {
                    RenderedElapsed::TenthsBoundary(elapsed.as_nanos())
                } else {
                    RenderedElapsed::TenthsOfSecond(millis / 50)
                }
            } else {
                RenderedElapsed::WholeSeconds(total_secs)
            }
        })
    }
}

impl StatusLine {
    /// Create a new status line.
    pub fn new() -> Self {
        Self {
            state: StatusState::new(),
            keybindings: ShellKeybindings::default(),
            enabled: true,
            spinner_frame: 0,
            last_spinner_update: Instant::now(),
            cached_content: Vec::new(),
            cached_state: None,
            cached_width: 0,
            cached_spinner_frame: None,
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

    pub fn set_keybindings(&mut self, keybindings: ShellKeybindings) {
        self.keybindings = keybindings;
        self.cached_state = None;
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

        // Capture elapsed time once so the cache key and the iocraft tree use
        // the exact same rendered value, even when a millisecond boundary is
        // crossed while drawing.
        let build_elapsed = self.state.build_start.map(|start| start.elapsed());
        let spinner_frame = self.state.building.then_some(self.spinner_frame);
        let cache_hit = self.cached_width == cols
            && self.cached_spinner_frame == spinner_frame
            && self
                .cached_state
                .as_ref()
                .is_some_and(|cached| cached.matches(&self.state, build_elapsed));

        if !cache_hit {
            self.cached_content.clear();
            let mut element = self.build_element_with_elapsed(cols, build_elapsed);
            let canvas = element.render(Some(cols as usize));
            canvas.write_ansi(&mut self.cached_content)?;
            // Keep only the first line. The terminal row is cleared below,
            // matching the previous behavior for wrapped canvas output.
            if let Some(pos) = self.cached_content.iter().position(|&b| b == b'\n') {
                self.cached_content.truncate(pos);
            }
            self.cached_width = cols;
            self.cached_spinner_frame = spinner_frame;
            self.cached_state = Some(CachedRenderState::from_state(&self.state, build_elapsed));
        }

        // Move to the last row, clear it, write content
        queue!(
            stdout,
            cursor::MoveTo(0, total_rows - 1),
            Clear(ClearType::CurrentLine)
        )?;
        stdout.write_all(&self.cached_content)?;
        queue!(stdout, ResetColor)?;

        Ok(())
    }

    /// Build the status line element.
    pub fn build_element(&self, width: u16) -> AnyElement<'static> {
        self.build_element_with_elapsed(width, self.state.build_start.map(|start| start.elapsed()))
    }

    /// Build the status line element using a captured elapsed duration.
    ///
    /// `draw` supplies this so its cache key and element are based on one
    /// instant. The public `build_element` retains its original behavior.
    fn build_element_with_elapsed(
        &self,
        width: u16,
        build_elapsed: Option<std::time::Duration>,
    ) -> AnyElement<'static> {
        // Use short keybind notation for narrow terminals
        let use_short = width < 60;

        if self.state.building {
            // Building state: spinner + elapsed time + changed files
            let spinner = self.spinner_char().to_string();
            let elapsed = build_elapsed.map(format_duration_parts);

            // Changed files inline
            let files_max_len = (width as usize).saturating_sub(40);
            let files_str = format_changed_files(&self.state.changed_files, files_max_len);
            let has_changed_files = !files_str.is_empty();

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding_left: 1, padding_right: 1) {
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0_f32, min_width: 0, overflow: Overflow::Hidden) {
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
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0_f32, min_width: 0, overflow: Overflow::Hidden) {
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
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0_f32, min_width: 0, overflow: Overflow::Hidden) {
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
            let keybind = self
                .keybindings
                .key_label(ShellAction::ToggleError, use_short);
            let error_action = if self.state.show_error {
                " hide error"
            } else {
                " show error"
            };

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding_left: 1, padding_right: 1) {
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0_f32, min_width: 0, overflow: Overflow::Hidden) {
                        View(margin_right: 1) {
                            Text(content: XMARK, color: COLOR_FAILED)
                        }
                        Text(content: "devenv ", color: COLOR_SECONDARY)
                        Text(content: "failed", weight: Weight::Bold, color: COLOR_FAILED)
                        #(duration)
                        #(watching)
                    }
                    View(flex_direction: FlexDirection::Row, flex_shrink: 0.0, margin_left: 2) {
                        #(if let Some(keybind) = keybind {
                            vec![
                                element!(Text(content: keybind, color: COLOR_INTERACTIVE)).into_any(),
                                element!(Text(content: error_action)).into_any(),
                            ]
                        } else {
                            vec![]
                        })
                    }
                }
            }
            .into_any()
        } else if self.state.paused {
            // Paused state
            let keybind = self
                .keybindings
                .key_label(ShellAction::TogglePause, use_short);

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding_left: 1, padding_right: 1) {
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0_f32, min_width: 0, overflow: Overflow::Hidden) {
                        View(margin_right: 2) {
                            Text(content: "⏸", color: COLOR_SECONDARY)
                        }
                        Text(content: "devenv ", color: COLOR_SECONDARY)
                        Text(content: "paused", weight: Weight::Bold, color: COLOR_ACTIVE)
                    }
                    View(flex_direction: FlexDirection::Row, flex_shrink: 0.0, margin_left: 2) {
                        #(if let Some(keybind) = keybind {
                            vec![
                                element!(Text(content: keybind, color: COLOR_INTERACTIVE)).into_any(),
                                element!(Text(content: " resume")).into_any(),
                            ]
                        } else {
                            vec![]
                        })
                    }
                }
            }
            .into_any()
        } else if self.state.watched_file_count > 0 {
            // Watching state
            let keybind = self
                .keybindings
                .key_label(ShellAction::TogglePause, use_short);
            let count_str = self.state.watched_file_count.to_string();

            element! {
                View(width: width as u32, height: 1, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding_left: 1, padding_right: 1) {
                    View(flex_direction: FlexDirection::Row, flex_grow: 1.0_f32, min_width: 0, overflow: Overflow::Hidden) {
                        View(margin_right: 2) {
                            Text(content: "👁", color: COLOR_SECONDARY)
                        }
                        Text(content: "devenv ", color: COLOR_SECONDARY)
                        Text(content: "watching ", weight: Weight::Bold, color: COLOR_ACTIVE)
                        Text(content: count_str, color: COLOR_COMPLETED)
                        Text(content: " files", color: COLOR_SECONDARY)
                    }
                    View(flex_direction: FlexDirection::Row, flex_shrink: 0.0, margin_left: 2) {
                        #(if let Some(keybind) = keybind {
                            vec![
                                element!(Text(content: keybind, color: COLOR_INTERACTIVE)).into_any(),
                                element!(Text(content: " pause")).into_any(),
                            ]
                        } else {
                            vec![]
                        })
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

    fn uncached_content(status_line: &StatusLine, width: u16) -> Vec<u8> {
        let mut element = status_line.build_element(width);
        let canvas = element.render(Some(width as usize));
        let mut content = Vec::new();
        canvas.write_ansi(&mut content).unwrap();
        if let Some(pos) = content.iter().position(|&byte| byte == b'\n') {
            content.truncate(pos);
        }
        content
    }

    fn draw_content(status_line: &mut StatusLine, width: u16) -> Vec<u8> {
        let mut output = Vec::new();
        status_line.draw(&mut output, width, 24).unwrap();
        output
    }

    fn assert_draw_matches_fresh_render(status_line: &mut StatusLine, width: u16) {
        let expected = uncached_content(status_line, width);
        let output = draw_content(status_line, width);
        assert_eq!(status_line.cached_content, expected);
        if !expected.is_empty() {
            assert!(
                output
                    .windows(expected.len())
                    .any(|window| window == expected.as_slice())
            );
        }
    }

    #[test]
    fn status_line_cache_reuses_identical_ansi_content() {
        let mut status_line = StatusLine::new();
        status_line.state_mut().set_paused(true);

        let expected = uncached_content(&status_line, 80);
        let first_output = draw_content(&mut status_line, 80);
        assert_eq!(status_line.cached_content, expected);

        let content_pointer = status_line.cached_content.as_ptr();
        let state_pointer = status_line.cached_state.as_ref().unwrap() as *const _;
        let second_output = draw_content(&mut status_line, 80);

        assert_eq!(second_output, first_output);
        assert_eq!(status_line.cached_content, expected);
        assert_eq!(status_line.cached_content.as_ptr(), content_pointer);
        assert_eq!(
            status_line.cached_state.as_ref().unwrap() as *const _,
            state_pointer
        );
    }

    #[test]
    fn status_line_uses_configured_bindings_and_hides_unbound_actions() {
        let mut keybindings = ShellKeybindings::default();
        keybindings.replace(
            ShellAction::TogglePause,
            vec![crate::keybindings::ShellKeyChord::new(
                crate::keybindings::ShellKeyCode::Function(12),
                false,
                false,
                false,
            )],
        );
        keybindings.replace(ShellAction::ToggleError, Vec::new());

        let mut status_line = StatusLine::new();
        status_line.set_keybindings(keybindings);
        status_line.state_mut().set_paused(true);
        let paused = String::from_utf8_lossy(&uncached_content(&status_line, 80)).into_owned();
        assert!(paused.contains("F12"));
        assert!(paused.contains(" resume"));

        status_line
            .state_mut()
            .set_build_failed(vec![PathBuf::from("devenv.nix")], "error".to_string());
        let failed = String::from_utf8_lossy(&uncached_content(&status_line, 80)).into_owned();
        assert!(!failed.contains("show error"));
        assert!(!failed.contains("hide error"));
    }

    #[test]
    fn status_line_cache_invalidates_for_state_width_and_spinner_changes() {
        let mut status_line = StatusLine::new();
        status_line.state_mut().set_paused(true);
        assert_draw_matches_fresh_render(&mut status_line, 80);
        let paused_content = status_line.cached_content.clone();

        status_line.state_mut().set_paused(false);
        status_line.state_mut().set_watched_file_count(2);
        assert_draw_matches_fresh_render(&mut status_line, 80);
        assert_ne!(status_line.cached_content, paused_content);
        assert!(matches!(
            status_line.cached_state,
            Some(CachedRenderState::Watching {
                watched_file_count: 2
            })
        ));

        assert_draw_matches_fresh_render(&mut status_line, 40);
        assert_eq!(status_line.cached_width, 40);
        assert_ne!(status_line.cached_content, paused_content);

        status_line
            .state_mut()
            .set_building(vec![PathBuf::from("devenv.nix")]);
        status_line.spinner_frame = 0;
        status_line.last_spinner_update = Instant::now();
        assert_draw_matches_fresh_render(&mut status_line, 80);
        let first_spinner_content = status_line.cached_content.clone();

        status_line.spinner_frame = 1;
        status_line.last_spinner_update = Instant::now();
        assert_draw_matches_fresh_render(&mut status_line, 80);
        assert_ne!(status_line.cached_content, first_spinner_content);
        assert_eq!(status_line.cached_spinner_frame, Some(1));
    }

    #[test]
    fn status_line_cache_matches_fresh_render_for_every_status_branch() {
        let mut status_line = StatusLine::new();
        assert_draw_matches_fresh_render(&mut status_line, 80);

        status_line.state_mut().set_watched_file_count(3);
        assert_draw_matches_fresh_render(&mut status_line, 80);

        status_line.state_mut().set_paused(true);
        assert_draw_matches_fresh_render(&mut status_line, 80);

        status_line.state_mut().set_paused(false);
        status_line.state_mut().error = Some("first error".into());
        status_line.state_mut().build_duration = Some(std::time::Duration::from_millis(250));
        assert_draw_matches_fresh_render(&mut status_line, 80);
        let failed_content = status_line.cached_content.clone();

        // Error text is intentionally not rendered; only its presence and the
        // expanded/collapsed action affect this branch.
        status_line.state_mut().error = Some("a different error".into());
        assert_draw_matches_fresh_render(&mut status_line, 80);
        assert_eq!(status_line.cached_content, failed_content);

        status_line.state_mut().show_error = true;
        assert_draw_matches_fresh_render(&mut status_line, 80);

        status_line.state_mut().error = None;
        status_line
            .state_mut()
            .set_reload_ready(vec![PathBuf::from("devenv.nix")]);
        assert_draw_matches_fresh_render(&mut status_line, 80);

        status_line.state_mut().set_reloaded();
        assert_draw_matches_fresh_render(&mut status_line, 80);

        status_line
            .state_mut()
            .set_building(vec![PathBuf::from("devenv.nix")]);
        status_line.state_mut().build_start = Some(
            Instant::now()
                .checked_sub(std::time::Duration::from_millis(2_100))
                .unwrap(),
        );
        status_line.last_spinner_update = Instant::now();
        assert_draw_matches_fresh_render(&mut status_line, 80);
    }

    #[test]
    fn rendered_elapsed_uses_display_precision() {
        let duration = |millis| Some(std::time::Duration::from_millis(millis));
        assert_eq!(
            rendered_elapsed(duration(999)),
            rendered_elapsed(duration(999))
        );

        if !cfg!(feature = "deterministic-tui") {
            assert_eq!(
                format_duration_parts(duration(1_101).unwrap()),
                format_duration_parts(duration(1_144).unwrap())
            );
            assert_eq!(
                rendered_elapsed(duration(1_101)),
                rendered_elapsed(duration(1_144))
            );
            assert_eq!(
                format_duration_parts(duration(1_144).unwrap()),
                format_duration_parts(duration(1_145).unwrap())
            );
            assert_ne!(
                rendered_elapsed(duration(1_144)),
                rendered_elapsed(duration(1_145))
            );
            assert_eq!(
                format_duration_parts(duration(1_149).unwrap()),
                format_duration_parts(duration(1_150).unwrap())
            );
            assert_ne!(
                rendered_elapsed(duration(1_149)),
                rendered_elapsed(duration(1_150))
            );
            assert_eq!(
                rendered_elapsed(duration(60_001)),
                rendered_elapsed(duration(60_999))
            );
            assert_eq!(
                format_duration_parts(duration(60_001).unwrap()),
                format_duration_parts(duration(60_999).unwrap())
            );
            assert_ne!(
                rendered_elapsed(duration(1_149)),
                rendered_elapsed(duration(1_200))
            );
        }
    }
}
