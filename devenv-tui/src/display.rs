use crate::{LogLevel, TuiEvent, TuiState};
use crossterm::{
    cursor, execute,
    style::Stylize,
    terminal::{size, Clear, ClearType},
};
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Default TUI display implementation that mimics CLI behavior
pub struct DefaultDisplay {
    event_receiver: mpsc::UnboundedReceiver<TuiEvent>,
    state: Arc<TuiState>,
    active_spinners: HashMap<crate::OperationId, SpinnerInfo>,
    spinner_frame: usize,
    last_spinner_update: Instant,
    active_lines: usize, // Total lines we're managing (spinners + completed)
}

/// Information about an active spinner
#[derive(Debug, Clone)]
struct SpinnerInfo {
    message: String,
    start_time: Instant,
    line_index: usize, // Which line this spinner starts at (0 = topmost)
    line_count: usize, // How many terminal lines this spinner occupies
}

/// Spinner animation frames (matching current CLI)
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

impl DefaultDisplay {
    pub fn new(event_receiver: mpsc::UnboundedReceiver<TuiEvent>, state: Arc<TuiState>) -> Self {
        Self {
            event_receiver,
            state,
            active_spinners: HashMap::new(),
            spinner_frame: 0,
            last_spinner_update: Instant::now(),
            active_lines: 0,
        }
    }

    /// Calculate how many terminal lines a message will occupy
    fn calculate_line_count(&self, message: &str) -> usize {
        let terminal_width = match size() {
            Ok((width, _)) => width as usize,
            Err(_) => 80, // Fallback to 80 columns
        };

        // Account for spinner character + space (2 chars)
        let available_width = terminal_width.saturating_sub(2);
        if available_width == 0 {
            return 1;
        }

        // Calculate lines needed for this message
        let message_len = message.chars().count();
        (message_len + available_width - 1) / available_width
    }

    /// Print a message normally (preserves spinner positions)
    fn print_message(&mut self, message: &str, level: LogLevel) {
        // If we have active spinner lines, clear them first
        if self.active_lines > 0 {
            self.clear_active_lines();
        }

        let symbol = match level {
            LogLevel::Error => "✖".red(),
            LogLevel::Warn => "•".yellow(),
            LogLevel::Info => "•".blue(),
            LogLevel::Debug => "•".italic(),
            LogLevel::Trace => "•".dim(),
        };

        println!("{} {}", symbol, message);

        // Redraw all active spinners below the message
        self.redraw_all_spinners();
    }

    /// Start a new spinner
    fn start_spinner(&mut self, id: crate::OperationId, message: String) {
        let line_count = self.calculate_line_count(&message);
        let line_index = self.active_lines;

        self.active_spinners.insert(
            id,
            SpinnerInfo {
                message: message.clone(),
                start_time: Instant::now(),
                line_index,
                line_count,
            },
        );

        // Print the initial spinner line
        let current_frame = SPINNER_FRAMES[self.spinner_frame % SPINNER_FRAMES.len()];
        print!("{} {}", current_frame.blue(), message);
        io::stdout().flush().ok();

        self.active_lines += line_count;
    }

    /// Complete a spinner by replacing its line with completion message
    fn complete_spinner(&mut self, id: &crate::OperationId, success: bool) {
        if let Some(_spinner) = self.active_spinners.remove(id) {
            let symbol = if success { "✓".green() } else { "✖".red() };
            let duration_str = format_duration(_spinner.start_time.elapsed());
            let completion_msg = format!("{} {} in {}", symbol, _spinner.message, duration_str);

            // Simple approach: clear current line and print completion message
            print!("\r{}\n", completion_msg);
            io::stdout().flush().ok();

            self.active_lines = 0;
            self.active_spinners.clear(); // For simplicity, clear all spinners
        }
    }

    /// Clear only the spinner lines, not completed messages
    fn clear_spinner_lines(&mut self) {
        if self.active_lines == 0 {
            return;
        }

        let mut stdout = io::stdout();

        // Clear each line that contains spinner content
        for i in 0..self.active_lines {
            let _ = execute!(
                stdout,
                cursor::MoveDown(i as u16),
                Clear(ClearType::CurrentLine),
                cursor::MoveUp(i as u16)
            );
        }

        let _ = stdout.flush();
    }

    /// Clear all managed lines
    fn clear_active_lines(&mut self) {
        if self.active_lines > 0 {
            let mut stdout = io::stdout();

            for i in 0..self.active_lines {
                let _ = execute!(
                    stdout,
                    cursor::MoveDown(i as u16),
                    Clear(ClearType::CurrentLine),
                    cursor::MoveUp(i as u16)
                );
            }

            // Move cursor to end
            let _ = execute!(stdout, cursor::MoveDown(self.active_lines as u16));
            let _ = stdout.flush();

            self.active_lines = 0;
        }
    }

    /// Redraw all active spinners
    fn redraw_all_spinners(&mut self) {
        if self.active_spinners.is_empty() {
            self.active_lines = 0;
            return;
        }

        let current_frame = SPINNER_FRAMES[self.spinner_frame % SPINNER_FRAMES.len()];
        let mut spinners: Vec<_> = self.active_spinners.values().collect();
        spinners.sort_by_key(|s| s.line_index);

        // Recalculate total active lines based on current spinners
        self.active_lines = spinners.iter().map(|s| s.line_count).sum();

        for spinner in spinners {
            println!("{} {}", current_frame.blue(), spinner.message);
        }

        // Move cursor back to manage position for spinner updates
        if self.active_lines > 0 {
            let mut stdout = io::stdout();
            let _ = execute!(stdout, cursor::MoveUp(self.active_lines as u16));
            let _ = stdout.flush();
        }
    }

    /// Update spinner animation frames in place
    fn update_spinner_frames(&mut self) {
        if self.active_spinners.is_empty() {
            return;
        }

        // Simple approach: just update the current spinner in place
        let current_frame = SPINNER_FRAMES[self.spinner_frame % SPINNER_FRAMES.len()];
        if let Some(spinner) = self.active_spinners.values().next() {
            print!("\r{} {}", current_frame.blue(), spinner.message);
            io::stdout().flush().ok();
        }
    }

    /// Main display loop
    pub async fn run(&mut self) {
        let mut spinner_ticker = tokio::time::interval(Duration::from_millis(100));

        loop {
            tokio::select! {
                // Handle TUI events
                event = self.event_receiver.recv() => {
                    match event {
                        Some(event) => {
                            self.handle_tui_event(event.clone());
                            self.state.handle_event(event);
                        }
                        None => break, // Channel closed
                    }
                }
                // Update spinners periodically
                _ = spinner_ticker.tick() => {
                    if self.last_spinner_update.elapsed() >= Duration::from_millis(100) {
                        self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
                        self.update_spinner_frames();
                        self.last_spinner_update = Instant::now();
                    }
                }
            }
        }
    }

    /// Handle individual TUI events
    fn handle_tui_event(&mut self, event: TuiEvent) {
        match event {
            TuiEvent::OperationStart { id, message, .. } => {
                self.start_spinner(id, message);
            }
            TuiEvent::OperationEnd { id, result, .. } => {
                let success = matches!(result, crate::OperationResult::Success);
                self.complete_spinner(&id, success);
            }
            TuiEvent::LogMessage { level, message, .. } => {
                self.print_message(&message, level);
            }
            _ => {} // Handle other events as needed
        }
    }
}

/// Format duration in human-readable format (matching current CLI)
fn format_duration(duration: Duration) -> String {
    let mut t = duration.as_nanos() as f64;
    for unit in ["ns", "µs", "ms", "s"].iter() {
        if t < 10.0 {
            return format!("{:.2}{}", t, unit);
        } else if t < 100.0 {
            return format!("{:.1}{}", t, unit);
        } else if t < 1000.0 {
            return format!("{:.0}{}", t, unit);
        }
        t /= 1000.0;
    }
    format!("{:.0}s", t * 1000.0)
}

/// Fallback display that works without TUI (similar to current indicatif setup)
pub struct FallbackDisplay {
    event_receiver: mpsc::UnboundedReceiver<TuiEvent>,
    state: Arc<TuiState>,
}

impl FallbackDisplay {
    pub fn new(event_receiver: mpsc::UnboundedReceiver<TuiEvent>, state: Arc<TuiState>) -> Self {
        Self {
            event_receiver,
            state,
        }
    }

    /// Simple console output without TUI
    pub async fn run(&mut self) {
        loop {
            while let Ok(event) = self.event_receiver.try_recv() {
                self.handle_event_simple(&event);
                self.state.handle_event(event);
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn handle_event_simple(&self, event: &TuiEvent) {
        match event {
            TuiEvent::OperationStart { message, .. } => {
                println!("⏵ {}", message);
            }
            TuiEvent::OperationEnd { result, .. } => match result {
                crate::OperationResult::Success => println!("✓ Complete"),
                crate::OperationResult::Failure { message, .. } => {
                    println!("✖ Failed: {}", message);
                }
            },
            TuiEvent::LogMessage {
                level,
                message,
                source,
                ..
            } => {
                let level_symbol = match level {
                    LogLevel::Error => "✖",
                    LogLevel::Warn => "⚠",
                    LogLevel::Info => "ℹ",
                    LogLevel::Debug => "🐛",
                    LogLevel::Trace => "📝",
                };

                let source_text = match source {
                    crate::LogSource::User => "",
                    crate::LogSource::Tracing => "[trace] ",
                    crate::LogSource::Nix => "[nix] ",
                    crate::LogSource::System => "[sys] ",
                };

                println!("{} {}{}", level_symbol, source_text, message);
            }
            _ => {} // Handle other events if needed
        }
    }
}
