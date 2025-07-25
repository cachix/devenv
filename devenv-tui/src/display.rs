use crate::{LogLevel, LogSource, OperationId, OperationResult, TuiEvent, TuiState};
use crossterm::{
    cursor, execute,
    style::Stylize,
    terminal::{size, Clear, ClearType},
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Gauge, Paragraph, Widget, Wrap},
    Frame, TerminalOptions, Viewport,
};
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Ratatui-based display using inline viewport for active region
pub struct RatatuiDisplay {
    event_receiver: mpsc::UnboundedReceiver<TuiEvent>,
    state: Arc<TuiState>,
    terminal:
        ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::BufWriter<std::io::Stderr>>>,
    active_operations: HashMap<OperationId, OperationWidget>,
    spinner_frame: usize,
    last_spinner_update: Instant,
    viewport_height: u16,
    min_viewport_height: u16,
    max_viewport_height: u16,
}

/// Widget information for an active operation
#[derive(Debug, Clone)]
struct OperationWidget {
    id: OperationId,
    message: String,
    start_time: Instant,
    parent: Option<OperationId>,
    widget_type: OperationWidgetType,
}

#[derive(Debug, Clone)]
enum OperationWidgetType {
    Spinner,
    Progress { current: u64, total: u64 },
}

/// Default TUI display implementation that mimics CLI behavior (kept for compatibility)
pub struct DefaultDisplay {
    event_receiver: mpsc::UnboundedReceiver<TuiEvent>,
    state: Arc<TuiState>,
    active_spinners: HashMap<OperationId, SpinnerInfo>,
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

impl RatatuiDisplay {
    pub fn new(
        event_receiver: mpsc::UnboundedReceiver<TuiEvent>,
        state: Arc<TuiState>,
    ) -> io::Result<Self> {
        // Initialize terminal manually without raw mode to allow signal propagation
        use ratatui::{backend::CrosstermBackend, Terminal};

        // Don't enable raw mode - we want signals to propagate normally
        let backend = CrosstermBackend::new(std::io::BufWriter::new(std::io::stderr()));
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(3), // Start small, will resize dynamically
            },
        )?;

        // Clear the terminal area without entering raw mode
        terminal.clear()?;

        Ok(Self {
            event_receiver,
            state,
            terminal,
            active_operations: HashMap::new(),
            spinner_frame: 0,
            last_spinner_update: Instant::now(),
            viewport_height: 3,
            min_viewport_height: 1,
            max_viewport_height: 15,
        })
    }

    /// Calculate optimal viewport height based on active operations
    fn calculate_viewport_height(&self) -> u16 {
        if self.active_operations.is_empty() {
            return self.min_viewport_height.max(1);
        }

        // Calculate total lines needed for all operations (accounting for text wrapping)
        let total_lines: usize = self
            .active_operations
            .values()
            .map(|op| {
                let line = Line::from(vec![
                    Span::raw("⠋ "), // Spinner character (2 chars: spinner + space)
                    Span::raw(&op.message),
                ]);
                let paragraph = Paragraph::new(line).wrap(Wrap { trim: true });
                // Use actual terminal width for calculation
                let width = crossterm::terminal::size().map(|(w, _)| w).unwrap_or(80);
                paragraph.line_count(width) as usize
            })
            .sum();

        let needed_height = total_lines.max(1) as u16;
        needed_height
            .max(self.min_viewport_height)
            .min(self.max_viewport_height)
    }

    /// Resize viewport if needed by creating a new terminal instance
    fn update_viewport_height(&mut self) -> io::Result<()> {
        let new_height = self.calculate_viewport_height();
        if new_height != self.viewport_height {
            self.viewport_height = new_height;

            // Create a new terminal with the updated viewport size
            use ratatui::{backend::CrosstermBackend, Terminal};
            let backend = CrosstermBackend::new(std::io::BufWriter::new(std::io::stderr()));
            let mut new_terminal = Terminal::with_options(
                backend,
                TerminalOptions {
                    viewport: Viewport::Inline(new_height),
                },
            )?;

            // Clear the new terminal area
            new_terminal.clear()?;

            // Swap the terminal instance
            self.terminal = new_terminal;
        }
        Ok(())
    }

    /// Print a log message above the active region
    fn print_log_message(
        &mut self,
        message: &str,
        level: LogLevel,
        source: LogSource,
    ) -> io::Result<()> {
        let symbol = match level {
            LogLevel::Error => "✖",
            LogLevel::Warn => "•",
            LogLevel::Info => "•",
            LogLevel::Debug => "•",
            LogLevel::Trace => "•",
        };

        let symbol_color = match level {
            LogLevel::Error => Color::Red,
            LogLevel::Warn => Color::Yellow,
            LogLevel::Info => Color::Blue,
            LogLevel::Debug => Color::Gray,
            LogLevel::Trace => Color::DarkGray,
        };

        let source_prefix = match source {
            LogSource::User => "",
            LogSource::Tracing => "[trace] ",
            LogSource::Nix => "[nix] ",
            LogSource::System => "[sys] ",
        };

        // Create spans with proper coloring - only symbol is colored
        let line = Line::from(vec![
            Span::styled(symbol, Style::default().fg(symbol_color)),
            Span::raw(format!(" {}{}", source_prefix, message)),
        ]);

        // Calculate how many lines this log message will need
        let paragraph = Paragraph::new(line.clone()).wrap(Wrap { trim: true });
        let width = crossterm::terminal::size().map(|(w, _)| w).unwrap_or(80);
        let line_count = paragraph.line_count(width);

        self.terminal
            .insert_before(line_count.try_into().unwrap_or(1), |buf| {
                paragraph.render(buf.area, buf);
            })?;
        Ok(())
    }

    /// Start a new operation widget
    fn start_operation(&mut self, id: OperationId, message: String, parent: Option<OperationId>) {
        let widget = OperationWidget {
            id: id.clone(),
            message,
            start_time: Instant::now(),
            parent,
            widget_type: OperationWidgetType::Spinner,
        };

        self.active_operations.insert(id, widget);
    }

    /// Complete an operation
    fn complete_operation(&mut self, id: &OperationId, success: bool) -> io::Result<()> {
        if let Some(operation) = self.active_operations.remove(id) {
            let symbol = if success { "✓" } else { "✖" };
            let symbol_color = if success { Color::Green } else { Color::Red };
            let duration_str = format_duration(operation.start_time.elapsed());

            // Create spans with proper coloring - only symbol is colored
            let line = Line::from(vec![
                Span::styled(symbol, Style::default().fg(symbol_color)),
                Span::raw(format!(" {} in {}", operation.message, duration_str)),
            ]);

            // Print completion message above the active region
            // Calculate how many lines this completion message will need
            let paragraph = Paragraph::new(line.clone()).wrap(Wrap { trim: true });
            let width = crossterm::terminal::size().map(|(w, _)| w).unwrap_or(80);
            let line_count = paragraph.line_count(width);

            self.terminal
                .insert_before(line_count.try_into().unwrap_or(1), |buf| {
                    paragraph.render(buf.area, buf);
                })?;

            // If no more active operations, clear the active region
            if self.active_operations.is_empty() {
                self.clear_active_region()?;
            }
        }
        Ok(())
    }

    /// Clear the active region when no operations are running
    fn clear_active_region(&mut self) -> io::Result<()> {
        self.terminal.draw(|frame| {
            // Render empty space to clear the region
            let area = frame.area();
            let empty = Paragraph::new("");
            frame.render_widget(empty, area);
        })?;
        Ok(())
    }

    /// Render the active region with current operations
    fn render_active_region(&mut self) -> io::Result<()> {
        // Force viewport height recalculation before rendering
        if let Err(e) = self.update_viewport_height() {
            return Err(e);
        }

        let active_operations = &self.active_operations;
        let spinner_frame = self.spinner_frame;

        self.terminal.draw(|frame| {
            Self::draw_operations(frame, active_operations, spinner_frame);
        })?;
        Ok(())
    }

    /// Draw active operations in the viewport
    fn draw_operations(
        frame: &mut Frame,
        active_operations: &HashMap<OperationId, OperationWidget>,
        spinner_frame: usize,
    ) {
        let area = frame.area();

        if active_operations.is_empty() {
            // Show a minimal status line when no operations are active
            let status = Paragraph::new("Ready").style(Style::default().fg(Color::Green));
            frame.render_widget(status, area);
            return;
        }

        // Create layout for operations - calculate lines needed for each
        let mut operations: Vec<_> = active_operations.values().collect();
        operations.sort_by_key(|op| &op.start_time);

        let constraints: Vec<Constraint> = operations
            .iter()
            .map(|op| {
                let line = Line::from(vec![
                    Span::raw("⠋ "), // Spinner character (2 chars: spinner + space)
                    Span::raw(&op.message),
                ]);
                let paragraph = Paragraph::new(line).wrap(Wrap { trim: true });
                let lines = paragraph.line_count(area.width);
                Constraint::Length(lines.try_into().unwrap_or(1))
            })
            .collect();

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        // operations are already filtered and sorted above

        for (i, operation) in operations.iter().enumerate() {
            if i < layout.len() {
                Self::draw_operation_widget(frame, layout[i], operation, spinner_frame);
            }
        }
    }

    /// Draw a single operation widget
    fn draw_operation_widget(
        frame: &mut Frame,
        area: Rect,
        operation: &OperationWidget,
        spinner_frame: usize,
    ) {
        match operation.widget_type {
            OperationWidgetType::Spinner => {
                let spinner_char = SPINNER_FRAMES[spinner_frame % SPINNER_FRAMES.len()];

                let line = Line::from(vec![
                    Span::styled(spinner_char, Style::default().fg(Color::Blue)),
                    Span::raw(format!(" {}", operation.message)),
                ]);

                let widget = Paragraph::new(line).wrap(Wrap { trim: true });

                frame.render_widget(widget, area);
            }
            OperationWidgetType::Progress { current, total } => {
                let progress = if total > 0 {
                    current as f64 / total as f64
                } else {
                    0.0
                };

                let widget = Gauge::default()
                    .block(Block::default().title(operation.message.clone()))
                    .gauge_style(Style::default().fg(Color::Blue))
                    .percent((progress * 100.0) as u16);

                frame.render_widget(widget, area);
            }
        }
    }

    /// Main display loop
    pub async fn run(&mut self) -> io::Result<()> {
        let mut spinner_ticker = tokio::time::interval(Duration::from_millis(50));

        // Set up signal handlers
        #[cfg(unix)]
        let mut sigint = {
            use tokio::signal::unix::{signal, SignalKind};
            signal(SignalKind::interrupt()).expect("Failed to register SIGINT handler")
        };
        #[cfg(unix)]
        let mut sigterm = {
            use tokio::signal::unix::{signal, SignalKind};
            signal(SignalKind::terminate()).expect("Failed to register SIGTERM handler")
        };

        let result = loop {
            #[cfg(unix)]
            {
                tokio::select! {
                    // Handle TUI events
                    event = self.event_receiver.recv() => {
                        match event {
                            Some(TuiEvent::Shutdown) => {
                                // Clean shutdown requested
                                break Ok(());
                            }
                            Some(event) => {
                                if let Err(e) = self.handle_tui_event(event.clone()) {
                                    break Err(e);
                                }
                                self.state.handle_event(event);
                            }
                            None => break Ok(()), // Channel closed
                        }
                    }
                    // Handle signals with access to active operations
                    _ = sigint.recv() => {
                        self.show_interrupted_operations()?;
                        crate::cleanup_tui();
                        std::process::exit(130); // Standard exit code for SIGINT
                    }
                    _ = sigterm.recv() => {
                        self.show_interrupted_operations()?;
                        crate::cleanup_tui();
                        std::process::exit(143); // Standard exit code for SIGTERM
                    }
                    // Update spinners and render periodically
                    _ = spinner_ticker.tick() => {
                        if self.last_spinner_update.elapsed() >= Duration::from_millis(50) {
                            self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
                            self.last_spinner_update = Instant::now();

                            // Update viewport height if needed
                            if let Err(e) = self.update_viewport_height() {
                                break Err(e);
                            }

                            // Render the active region
                            if let Err(e) = self.render_active_region() {
                                break Err(e);
                            }
                        }
                    }
                }
            }
            #[cfg(not(unix))]
            {
                tokio::select! {
                    // Handle TUI events
                    event = self.event_receiver.recv() => {
                        match event {
                            Some(TuiEvent::Shutdown) => {
                                // Clean shutdown requested
                                break Ok(());
                            }
                            Some(event) => {
                                if let Err(e) = self.handle_tui_event(event.clone()) {
                                    break Err(e);
                                }
                                self.state.handle_event(event);
                            }
                            None => break Ok(()), // Channel closed
                        }
                    }
                    // Update spinners and render periodically
                    _ = spinner_ticker.tick() => {
                        if self.last_spinner_update.elapsed() >= Duration::from_millis(50) {
                            self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
                            self.last_spinner_update = Instant::now();

                            // Update viewport height if needed
                            if let Err(e) = self.update_viewport_height() {
                                break Err(e);
                            }

                            // Render the active region
                            if let Err(e) = self.render_active_region() {
                                break Err(e);
                            }
                        }
                    }
                }
            }
        };

        // Always cleanup terminal state on exit
        crate::cleanup_tui();
        result
    }

    /// Show interrupted state for all active operations
    fn show_interrupted_operations(&mut self) -> io::Result<()> {
        if self.active_operations.is_empty() {
            return Ok(());
        }

        self.terminal.draw(|frame| {
            Self::draw_interrupted_operations(frame, &self.active_operations);
        })?;
        Ok(())
    }

    /// Draw interrupted operations with gray pause icons
    fn draw_interrupted_operations(
        frame: &mut Frame,
        active_operations: &HashMap<OperationId, OperationWidget>,
    ) {
        let area = frame.area();

        if active_operations.is_empty() {
            return;
        }

        // Create layout for operations - same logic as normal rendering
        let mut operations: Vec<_> = active_operations.values().collect();
        operations.sort_by_key(|op| &op.start_time);

        let constraints: Vec<Constraint> = operations
            .iter()
            .map(|op| {
                let line = Line::from(vec![
                    Span::raw("⏸ "), // Interrupted icon
                    Span::raw(&op.message),
                ]);
                let paragraph = Paragraph::new(line).wrap(Wrap { trim: true });
                let lines = paragraph.line_count(area.width);
                Constraint::Length(lines.try_into().unwrap_or(1))
            })
            .collect();

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        for (i, operation) in operations.iter().enumerate() {
            if i < layout.len() {
                // Render with interrupted icon
                let line = Line::from(vec![
                    Span::styled("⏸", Style::default().fg(Color::Yellow)),
                    Span::raw(format!(" {}", operation.message)),
                ]);

                let widget = Paragraph::new(line).wrap(Wrap { trim: true });
                frame.render_widget(widget, layout[i]);
            }
        }
    }

    /// Handle individual TUI events
    fn handle_tui_event(&mut self, event: TuiEvent) -> io::Result<()> {
        match event {
            TuiEvent::OperationStart {
                id,
                message,
                parent,
                ..
            } => {
                self.start_operation(id, message, parent);
            }
            TuiEvent::OperationEnd { id, result, .. } => {
                let success = matches!(result, OperationResult::Success);
                self.complete_operation(&id, success)?;
            }
            TuiEvent::LogMessage {
                level,
                message,
                source,
                ..
            } => {
                self.print_log_message(&message, level, source)?;
            }
            _ => {} // Handle other events as needed
        }
        Ok(())
    }
}

impl Drop for RatatuiDisplay {
    fn drop(&mut self) {
        // Ensure terminal is always restored
        crate::cleanup_tui();
    }
}

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
        let mut spinner_ticker = tokio::time::interval(Duration::from_millis(50));

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
                    if self.last_spinner_update.elapsed() >= Duration::from_millis(50) {
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
