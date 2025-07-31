use crate::{
    graph_display::GraphDisplay, LogLevel, OperationId, OperationResult, TuiEvent, TuiState,
};
use crossterm::{
    cursor,
    event::{Event, KeyCode},
    execute,
    style::Stylize,
    terminal::{size, Clear, ClearType},
};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
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
    graph_display: GraphDisplay,
}

/// Widget information for an active operation
#[derive(Debug, Clone)]
struct OperationWidget {
    message: String,
    start_time: Instant,
    _parent: Option<OperationId>,
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
        // Initialize terminal with raw mode for proper keyboard handling
        use ratatui::{backend::CrosstermBackend, Terminal};

        // Enable raw mode for proper keyboard input handling
        crossterm::terminal::enable_raw_mode()?;

        // Make sure cursor is visible before initialization
        let _ = crossterm::execute!(std::io::stderr(), crossterm::cursor::Show);

        // Create backend
        let backend = CrosstermBackend::new(std::io::BufWriter::new(std::io::stderr()));

        // Try to create terminal with inline viewport
        let terminal = match Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(20), // Fixed size to avoid cursor issues
            },
        ) {
            Ok(term) => {
                // Don't clear - we want to preserve existing terminal content with inline viewport
                term
            }
            Err(e) => {
                // If inline viewport fails, return the error to trigger fallback
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("Terminal initialization failed: {}", e),
                ));
            }
        };

        let graph_display = GraphDisplay::new(state.clone());

        Ok(Self {
            event_receiver,
            state,
            terminal,
            active_operations: HashMap::new(),
            spinner_frame: 0,
            last_spinner_update: Instant::now(),
            viewport_height: 20,
            min_viewport_height: 10,
            max_viewport_height: 100,
            graph_display,
        })
    }

    /// Calculate optimal viewport height based on active operations
    fn calculate_viewport_height(&self) -> u16 {
        // Get all activities (not just operations)
        let activities = self.graph_display.get_all_activities();

        if activities.is_empty() {
            return self.min_viewport_height;
        }

        // Base height: activities + summary (5 lines)
        let mut desired_height = activities.len() as u16 + 5;

        // Add space for logs if something is selected
        if self.graph_display.has_selection() {
            desired_height += 15; // Add space for log viewer
        }

        // Clamp to min/max bounds
        desired_height
            .min(self.max_viewport_height)
            .max(self.min_viewport_height)
    }

    /// Resize viewport if needed by creating a new terminal instance
    fn update_viewport_height(&mut self) -> io::Result<()> {
        // The terminal will automatically resize during draw() based on content
        // We just need to track the desired height for our calculations
        self.viewport_height = self.calculate_viewport_height();
        Ok(())
    }

    /// Start a new operation widget
    fn start_operation(&mut self, id: OperationId, message: String, parent: Option<OperationId>) {
        let widget = OperationWidget {
            message,
            start_time: Instant::now(),
            _parent: parent,
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
        // Use the graph display for rendering
        self.terminal.draw(|frame| {
            self.graph_display.render(frame.area(), frame.buffer_mut());
        })?;
        Ok(())
    }

    /// Main display loop
    pub async fn run(&mut self) -> io::Result<()> {
        let mut spinner_ticker = tokio::time::interval(Duration::from_millis(50));

        // Create keyboard event stream
        use futures::StreamExt;
        let mut event_stream = crossterm::event::EventStream::new();

        // Set up signal handler for Ctrl-C
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigint =
            signal(SignalKind::interrupt()).expect("Failed to register SIGINT handler");

        let result = loop {
            tokio::select! {
            // Handle TUI events
            event = self.event_receiver.recv() => {
                match event {
                    Some(TuiEvent::Shutdown) => {
                        // Clean shutdown requested - show interrupted operations
                        self.show_interrupted_operations()?;
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
            // Handle keyboard events
            keyboard_event = event_stream.next() => {
                if let Some(Ok(Event::Key(key))) = keyboard_event {
                    match key.code {
                        KeyCode::Up => {
                            self.graph_display.select_previous();
                            if let Err(e) = self.render_active_region() {
                                break Err(e);
                            }
                        }
                        KeyCode::Down => {
                            self.graph_display.select_next();
                            if let Err(e) = self.render_active_region() {
                                break Err(e);
                            }
                        }
                        KeyCode::Esc => {
                            // Clear selection instead of quitting
                            self.graph_display.clear_selection();
                            if let Err(e) = self.render_active_region() {
                                break Err(e);
                            }
                        }
                        KeyCode::Char('c') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                            // Handle Ctrl-C as keyboard event
                            self.show_interrupted_operations()?;
                            // Send shutdown event to break out of the loop
                            if let Ok(sender) = crate::GLOBAL_SENDER.lock() {
                                if let Some(tx) = sender.as_ref() {
                                    let _ = tx.send(TuiEvent::Shutdown);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            // Handle Ctrl-C (SIGINT)
            _ = sigint.recv() => {
                // Disable raw mode first to ensure terminal is responsive
                let _ = crossterm::terminal::disable_raw_mode();
                self.show_interrupted_operations()?;
                break Ok(());
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
        };

        // Always cleanup terminal state on exit
        crate::cleanup_tui();
        result
    }

    /// Show interrupted state for all active operations
    fn show_interrupted_operations(&mut self) -> io::Result<()> {
        // Show final summary instead of active operations
        let graph_display = &mut self.graph_display;

        self.terminal.draw(|frame| {
            let area = frame.area();

            // Clear the screen
            frame.render_widget(ratatui::widgets::Clear, area);

            // Render the full graph display which will show the summary
            graph_display.render(area, frame.buffer_mut());
        })?;
        Ok(())
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
            _ => {} // Handle other events as needed
        }
        Ok(())
    }
}

impl Drop for RatatuiDisplay {
    fn drop(&mut self) {
        // Ensure terminal is always restored
        let _ = crossterm::terminal::disable_raw_mode();
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
            TuiEvent::NixDerivationStart {
                derivation_name,
                machine,
                ..
            } => {
                let machine_text = machine.map(|m| format!(" on {}", m)).unwrap_or_default();
                let message = format!("Building {}{}", derivation_name, machine_text);
                self.print_message(&message, LogLevel::Info);
            }
            TuiEvent::NixPhaseProgress { phase, .. } => {
                let message = format!("Phase: {}", phase);
                self.print_message(&message, LogLevel::Debug);
            }
            TuiEvent::NixDownloadStart {
                package_name,
                substituter,
                ..
            } => {
                let message = format!("Downloading {} from {}", package_name, substituter);
                self.print_message(&message, LogLevel::Info);
            }
            TuiEvent::NixDownloadProgress {
                activity_id,
                bytes_downloaded,
                total_bytes,
                ..
            } => {
                let download_info = self.state.get_nix_download(activity_id);

                let message = if let Some(info) = download_info {
                    if let Some(total) = total_bytes {
                        let percent = (bytes_downloaded as f64 / total as f64 * 100.0) as u32;
                        format!(
                            "Download progress: {}% ({}/{}) - {}",
                            percent,
                            format_bytes(bytes_downloaded),
                            format_bytes(total),
                            format_speed(info.download_speed)
                        )
                    } else {
                        format!(
                            "Downloaded: {} - {}",
                            format_bytes(bytes_downloaded),
                            format_speed(info.download_speed)
                        )
                    }
                } else {
                    // Fallback
                    if let Some(total) = total_bytes {
                        let percent = (bytes_downloaded as f64 / total as f64 * 100.0) as u32;
                        format!(
                            "Download progress: {}% ({}/{})",
                            percent,
                            format_bytes(bytes_downloaded),
                            format_bytes(total)
                        )
                    } else {
                        format!("Downloaded: {}", format_bytes(bytes_downloaded))
                    }
                };
                self.print_message(&message, LogLevel::Debug);
            }
            TuiEvent::NixActivityProgress { .. } => {
                // Progress updates are displayed in the spinners, no need for specific handling here
            }
            _ => {} // Handle other events as needed
        }
    }
}

/// Format duration in human-readable format (matching current CLI)
pub fn format_duration(duration: Duration) -> String {
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

/// Format bytes in human-readable format
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else if size < 10.0 {
        format!("{:.2} {}", size, UNITS[unit_index])
    } else if size < 100.0 {
        format!("{:.1} {}", size, UNITS[unit_index])
    } else {
        format!("{:.0} {}", size, UNITS[unit_index])
    }
}

/// Format download speed in human-readable format
pub fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec <= 0.0 {
        return "0 B/s".to_string();
    }

    const UNITS: &[&str] = &["B/s", "KiB/s", "MiB/s", "GiB/s", "TiB/s"];
    let mut speed = bytes_per_sec;
    let mut unit_index = 0;

    while speed >= 1024.0 && unit_index < UNITS.len() - 1 {
        speed /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{:.0} {}", speed, UNITS[unit_index])
    } else if speed < 10.0 {
        format!("{:.2} {}", speed, UNITS[unit_index])
    } else if speed < 100.0 {
        format!("{:.1} {}", speed, UNITS[unit_index])
    } else {
        format!("{:.0} {}", speed, UNITS[unit_index])
    }
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
