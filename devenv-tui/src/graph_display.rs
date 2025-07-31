use crate::{
    ActivityProgress, NixActivityState, NixActivityType, NixDerivationInfo, NixDownloadInfo,
    NixQueryInfo, OperationId, TuiState,
};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{LineGauge, Paragraph, Widget},
};
use std::sync::Arc;
use std::time::Instant;

/// Graph display widget that shows active Nix activities with a sticky summary
pub struct GraphDisplay {
    state: Arc<TuiState>,
    selected_index: Option<usize>,
    scroll_offset: usize,
    scroll_position: usize,
    activities_visible_height: u16, // Track actual visible height for activities
}

impl GraphDisplay {
    pub fn new(state: Arc<TuiState>) -> Self {
        Self {
            state,
            selected_index: None,
            scroll_offset: 0,
            scroll_position: 0,
            activities_visible_height: 5, // Default, will be updated during render
        }
    }

    /// Check if an item is selected
    pub fn has_selection(&self) -> bool {
        self.selected_index.is_some()
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        let activities = self.get_all_activities();
        if activities.is_empty() {
            return;
        }

        // Find the previous build activity
        let start_index = self.selected_index.unwrap_or(activities.len());

        // Search backwards from current position
        for i in (0..start_index).rev() {
            if activities[i].activity_type == NixActivityType::Build {
                self.selected_index = Some(i);
                // Adjust scroll if needed
                if i < self.scroll_offset {
                    self.scroll_offset = i;
                }
                return;
            }
        }

        // If no build found before current position, wrap to the end and search backwards
        if self.selected_index.is_some() {
            for i in (start_index..activities.len()).rev() {
                if activities[i].activity_type == NixActivityType::Build {
                    self.selected_index = Some(i);
                    // Adjust scroll to show the item
                    let visible_height = self.activities_visible_height as usize;
                    self.scroll_offset = i.saturating_sub(visible_height - 1);
                    return;
                }
            }
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        let activities = self.get_all_activities();
        if activities.is_empty() {
            return;
        }

        // Find the next build activity
        let start_index = self.selected_index.map(|i| i + 1).unwrap_or(0);

        // Search forward from current position
        for i in start_index..activities.len() {
            if activities[i].activity_type == NixActivityType::Build {
                self.selected_index = Some(i);
                // Adjust scroll if needed
                let visible_height = self.activities_visible_height as usize;
                if i >= self.scroll_offset + visible_height {
                    self.scroll_offset = i.saturating_sub(visible_height - 1);
                }
                return;
            }
        }

        // If no build found after current position, wrap to the beginning
        if self.selected_index.is_some() {
            for i in 0..start_index.min(activities.len()) {
                if activities[i].activity_type == NixActivityType::Build {
                    self.selected_index = Some(i);
                    self.scroll_offset = 0;
                    return;
                }
            }
        }
    }

    /// Clear selection
    pub fn clear_selection(&mut self) {
        self.selected_index = None;
        self.scroll_position = 0;
    }

    /// Scroll logs up
    pub fn scroll_logs_up(&mut self) {
        if self.scroll_position > 0 {
            self.scroll_position -= 1;
        }
    }

    /// Scroll logs down
    pub fn scroll_logs_down(&mut self) {
        self.scroll_position += 1;
    }

    /// Get all active activities sorted by hierarchy and start time
    pub fn get_all_activities(&self) -> Vec<ActivityInfo> {
        let mut activities = Vec::new();

        // Get root operations (no parent)
        let root_operations = self.state.get_root_operations();

        // Build tree recursively starting from roots
        for root_op in root_operations {
            self.add_operation_activities(&mut activities, &root_op, 0);
        }

        activities
    }

    /// Recursively add activities for an operation and its children
    fn add_operation_activities(
        &self,
        activities: &mut Vec<ActivityInfo>,
        operation: &crate::Operation,
        depth: usize,
    ) {
        // Always show the operation itself if it's active
        if matches!(operation.state, crate::OperationState::Active) {
            activities.push(ActivityInfo {
                activity_type: NixActivityType::Evaluating,
                activity_id: None,
                name: operation.message.clone(),
                details: String::new(),
                start_time: operation.start_time,
                progress: None,
                generic_progress: None,
                current_phase: None,
                download_speed: None,
                bytes_downloaded: None,
                total_bytes: None,
                depth,
                evaluation_count: operation.data.get("evaluation_count").cloned(),
            });
        }

        // Get all Nix activities for this operation
        let (derivations, downloads, queries) = self
            .state
            .get_all_nix_activities_for_operation(&operation.id);

        // Add active derivations or derivations with logs
        for deriv in &derivations {
            let has_logs = !self.state.get_build_logs(deriv.activity_id).is_empty();
            if deriv.state == NixActivityState::Active || has_logs {
                let mut activity = ActivityInfo::from_derivation(
                    deriv.clone(),
                    operation.parent.clone(),
                    depth + 1,
                );
                // Get generic progress if available
                activity.generic_progress = self.state.get_activity_progress(deriv.activity_id);
                activities.push(activity);
            }
        }

        // Add active downloads
        for download in &downloads {
            if download.state == NixActivityState::Active {
                let mut activity = ActivityInfo::from_download(
                    download.clone(),
                    operation.parent.clone(),
                    depth + 1,
                );
                // Get generic progress if available
                activity.generic_progress = self.state.get_activity_progress(download.activity_id);
                activities.push(activity);
            }
        }

        // Add active queries
        for query in &queries {
            if query.state == NixActivityState::Active {
                let mut activity =
                    ActivityInfo::from_query(query.clone(), operation.parent.clone(), depth + 1);
                // Get generic progress if available
                activity.generic_progress = self.state.get_activity_progress(query.activity_id);
                activities.push(activity);
            }
        }

        // Recursively add children
        let children = self.state.get_children(&operation.id);
        for child in children {
            self.add_operation_activities(activities, &child, depth + 1);
        }
    }

    /// Calculate summary statistics
    fn calculate_summary(&self) -> SummaryStats {
        let mut stats = SummaryStats::default();

        // Get all activities across all operations
        let operations = self.state.get_active_operations();

        for op in &operations {
            // Get all Nix activities for this operation
            let (derivations, downloads, queries) =
                self.state.get_all_nix_activities_for_operation(&op.id);

            // Count builds by state
            for deriv in derivations {
                match deriv.state {
                    NixActivityState::Active => stats.builds_running += 1,
                    NixActivityState::Completed { success: true, .. } => {
                        stats.builds_completed += 1
                    }
                    NixActivityState::Completed { success: false, .. } => stats.builds_failed += 1,
                }
            }

            // Count downloads by state and calculate speed
            for download in downloads {
                match download.state {
                    NixActivityState::Active => {
                        stats.downloads_running += 1;
                        stats.total_download_speed += download.download_speed;
                    }
                    NixActivityState::Completed { success: true, .. } => {
                        stats.downloads_completed += 1
                    }
                    NixActivityState::Completed { success: false, .. } => {
                        stats.downloads_failed += 1
                    }
                }
            }

            // Count queries by state
            for query in queries {
                match query.state {
                    NixActivityState::Active => stats.queries_running += 1,
                    NixActivityState::Completed { success: true, .. } => {
                        stats.queries_completed += 1
                    }
                    NixActivityState::Completed { success: false, .. } => stats.queries_failed += 1,
                }
            }
        }

        stats
    }
}

impl Widget for &mut GraphDisplay {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let activities = self.get_all_activities();

        // Check if we should show logs (only for builds when selected)
        let selected_build = if let Some(selected_idx) = self.selected_index {
            if selected_idx < activities.len()
                && activities[selected_idx].activity_type == NixActivityType::Build
            {
                Some(&activities[selected_idx])
            } else {
                None
            }
        } else {
            None
        };

        // Layout: Always have 3 sections - logs (if selected), activities, and summary
        // Use Min(1) for activities to allow it to use remaining space after summary
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(if selected_build.is_some() {
                vec![
                    Constraint::Length(20), // Build logs area (reduced to 20 lines)
                    Constraint::Min(1),     // Graph area uses remaining space
                    Constraint::Length(5),  // Summary area (always visible, fixed height)
                ]
            } else {
                vec![
                    Constraint::Length(0), // No logs area
                    Constraint::Min(1),    // Graph area uses remaining space
                    Constraint::Length(5), // Summary area (always visible, fixed height)
                ]
            })
            .split(area);

        // Render build logs at the top if a build is selected
        if let Some(build_activity) = selected_build {
            if let Some(activity_id) = build_activity.activity_id {
                let build_logs = self.state.get_build_logs(activity_id);

                // Show up to 50 lines of logs (or however many fit in the area)
                let log_area = chunks[0];
                let max_lines = log_area.height as usize;
                let log_lines: Vec<Line> = build_logs
                    .iter()
                    .rev()
                    .take(max_lines)
                    .rev()
                    .map(|log| Line::from(log.as_str()))
                    .collect();

                if log_lines.is_empty() {
                    let waiting_msg = Line::from(vec![Span::styled(
                        "Waiting for build output...",
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    )]);
                    let paragraph = Paragraph::new(waiting_msg);
                    paragraph.render(log_area, buf);
                } else {
                    let paragraph =
                        Paragraph::new(log_lines).style(Style::default().fg(Color::Gray));
                    paragraph.render(log_area, buf);
                }

                // Add separator after logs
                if log_area.bottom() < area.bottom() {
                    let separator = "─".repeat(area.width as usize);
                    let separator_line = Line::from(Span::styled(
                        separator,
                        Style::default().fg(Color::DarkGray),
                    ));
                    let separator_area = Rect {
                        x: area.x,
                        y: log_area.bottom(),
                        width: area.width,
                        height: 1,
                    };
                    Paragraph::new(separator_line).render(separator_area, buf);
                }
            }
        }

        // Always render activities in the middle section (chunks[1])
        // Store the visible height for scroll calculations
        let activities_area = chunks[1];
        self.activities_visible_height = activities_area.height;

        let graph_widget = GraphWidget {
            activities: &activities,
            selected_index: self.selected_index,
            scroll_offset: self.scroll_offset,
        };
        graph_widget.render(activities_area, buf);

        // Always render summary at the bottom (chunks[2])
        let summary = self.calculate_summary();
        let summary_widget = SummaryWidget {
            stats: summary,
            has_selection: self.selected_index.is_some(),
        };
        summary_widget.render(chunks[2], buf);
    }
}

/// Widget for the main activity graph
struct GraphWidget<'a> {
    activities: &'a [ActivityInfo],
    selected_index: Option<usize>,
    scroll_offset: usize,
}

impl<'a> Widget for GraphWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // No borders for a cleaner look
        // Add a small margin on the left
        let inner = Rect {
            x: area.x + 1,
            y: area.y,
            width: area.width.saturating_sub(1),
            height: area.height,
        };

        // Calculate visible range
        let visible_height = inner.height as usize;
        let visible_activities = self
            .activities
            .iter()
            .skip(self.scroll_offset)
            .take(visible_height)
            .enumerate();

        // Render each activity
        if self.activities.is_empty() {
            // Show a message when no activities are running
            let no_activities_msg = Line::from(vec![Span::styled(
                "No active operations",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )]);
            let paragraph = Paragraph::new(no_activities_msg);
            paragraph.render(inner, buf);
        } else {
            for (display_index, (actual_index, activity)) in visible_activities.enumerate() {
                let y = inner.y + display_index as u16;
                if y < inner.bottom() {
                    let is_selected =
                        self.selected_index == Some(actual_index + self.scroll_offset);
                    activity.render_line(inner.x, y, inner.width, is_selected, buf);
                }
            }
        }
    }
}

/// Widget for the summary section
struct SummaryWidget {
    stats: SummaryStats,
    has_selection: bool,
}

impl Widget for SummaryWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // No borders for a cleaner look
        // Add a separator line at the top
        if area.height > 0 {
            let separator = "─".repeat(area.width as usize);
            let separator_line = Line::from(Span::styled(
                separator,
                Style::default().fg(Color::DarkGray),
            ));
            let separator_area = Rect {
                x: area.x,
                y: area.y,
                width: area.width,
                height: 1,
            };
            Paragraph::new(separator_line).render(separator_area, buf);
        }

        // Render summary content below separator
        let content_area = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(1),
            height: area.height.saturating_sub(1),
        };

        // Split the content area into left (summary) and right (help) sections
        let split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(50),    // Summary stats
                Constraint::Length(40), // Help text
            ])
            .split(content_area);

        // Left side: Summary stats
        let summary_lines = vec![
            self.format_summary_line(
                "Queries",
                self.stats.queries_running,
                self.stats.queries_completed,
            ),
            self.format_summary_line(
                "Downloads",
                self.stats.downloads_running,
                self.stats.downloads_completed,
            ),
            self.format_summary_line(
                "Builds",
                self.stats.builds_running,
                self.stats.builds_completed,
            ),
        ];

        let summary_paragraph = Paragraph::new(summary_lines);
        summary_paragraph.render(split[0], buf);

        // Right side: Help text
        if split.len() > 1 && split[1].width > 10 {
            let help_lines = if self.has_selection {
                vec![
                    Line::from(vec![
                        Span::styled("↑↓", Style::default().fg(Color::Yellow)),
                        Span::raw(" navigate"),
                    ]),
                    Line::from(vec![
                        Span::styled("Esc", Style::default().fg(Color::Yellow)),
                        Span::raw(" deselect"),
                    ]),
                ]
            } else {
                vec![Line::from(vec![
                    Span::styled("↑↓", Style::default().fg(Color::Yellow)),
                    Span::raw(" show build logs"),
                ])]
            };

            let help_paragraph = Paragraph::new(help_lines).alignment(Alignment::Right);
            help_paragraph.render(split[1], buf);
        }
    }
}

impl SummaryWidget {
    fn format_summary_line(&self, category: &str, running: usize, done: usize) -> Line {
        let mut spans = vec![];

        // Calculate max width needed for each column
        let max_running = self
            .stats
            .queries_running
            .max(self.stats.downloads_running)
            .max(self.stats.builds_running);
        let max_done = self
            .stats
            .queries_completed
            .max(self.stats.downloads_completed)
            .max(self.stats.builds_completed);

        let running_width = max_running.to_string().len().max(1);
        let done_width = max_done.to_string().len().max(1);

        // Category name (right-padded to 10 chars for alignment)
        spans.push(Span::raw(format!("{:>10}: ", category)));

        // Running count with blue color (right-aligned)
        spans.push(Span::raw(format!(
            "{:>width$} ",
            running,
            width = running_width
        )));
        spans.push(Span::styled("running", Style::default().fg(Color::Blue)));

        spans.push(Span::raw(" "));

        // Done count with green color (right-aligned)
        spans.push(Span::raw(format!("{:>width$} ", done, width = done_width)));
        spans.push(Span::styled("done", Style::default().fg(Color::Green)));

        Line::from(spans)
    }
}

/// Unified activity information
#[derive(Clone)]
pub struct ActivityInfo {
    activity_type: NixActivityType,
    activity_id: Option<u64>, // For builds/downloads/queries that have activity IDs
    name: String,
    details: String,
    start_time: Instant,
    progress: Option<f64>,
    generic_progress: Option<ActivityProgress>, // New field for generic progress
    current_phase: Option<String>,
    download_speed: Option<f64>,
    bytes_downloaded: Option<u64>,
    total_bytes: Option<u64>,
    depth: usize,
    evaluation_count: Option<String>,
}

impl ActivityInfo {
    fn from_derivation(
        deriv: NixDerivationInfo,
        _parent: Option<OperationId>,
        depth: usize,
    ) -> Self {
        Self {
            activity_type: NixActivityType::Build,
            activity_id: Some(deriv.activity_id),
            name: deriv.derivation_name,
            details: String::new(), // Remove machine info
            start_time: deriv.start_time,
            progress: None,
            generic_progress: None,
            current_phase: deriv.current_phase,
            download_speed: None,
            bytes_downloaded: None,
            total_bytes: None,
            depth,
            evaluation_count: None,
        }
    }

    fn from_download(
        download: NixDownloadInfo,
        _parent: Option<OperationId>,
        depth: usize,
    ) -> Self {
        let progress = download
            .total_bytes
            .map(|total| download.bytes_downloaded as f64 / total as f64);

        Self {
            activity_type: NixActivityType::Download,
            activity_id: Some(download.activity_id),
            name: download.package_name,
            details: download.substituter,
            start_time: download.start_time,
            progress,
            generic_progress: None,
            current_phase: None,
            download_speed: Some(download.download_speed),
            bytes_downloaded: Some(download.bytes_downloaded),
            total_bytes: download.total_bytes,
            depth,
            evaluation_count: None,
        }
    }

    fn from_query(query: NixQueryInfo, _parent: Option<OperationId>, depth: usize) -> Self {
        Self {
            activity_type: NixActivityType::Query,
            activity_id: Some(query.activity_id),
            name: query.package_name,
            details: query.substituter,
            start_time: query.start_time,
            progress: None,
            generic_progress: None,
            current_phase: None,
            download_speed: None,
            bytes_downloaded: None,
            total_bytes: None,
            depth,
            evaluation_count: None,
        }
    }

    /// Render a single line for this activity
    fn render_line(&self, x: u16, y: u16, width: u16, is_selected: bool, buf: &mut Buffer) {
        let elapsed = self.start_time.elapsed();
        let elapsed_str = crate::display::format_duration(elapsed);

        // Activity type word (all in-progress activities use blue)
        let type_word = match self.activity_type {
            NixActivityType::Build => "Building",
            NixActivityType::Download => "Downloading",
            NixActivityType::Query => "Querying",
            NixActivityType::Evaluating => "Evaluating",
            NixActivityType::Unknown => "Processing",
        };
        let color = Color::Blue; // All in-progress activities are blue

        // Build the line with indentation for hierarchy
        let mut spans = vec![];

        // Always start with selection indicator space (keeps alignment consistent)
        if is_selected {
            spans.push(Span::styled(
                "▶ ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::raw("  ")); // Keep alignment
        }

        // Add indentation based on depth
        if self.depth > 0 {
            let indent = "  ".repeat(self.depth);
            spans.push(Span::raw(indent));
            spans.push(Span::raw("└─ "));
        }

        let style_modifier = if is_selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        };
        spans.push(Span::styled(
            type_word,
            Style::default()
                .fg(color)
                .add_modifier(Modifier::BOLD | style_modifier),
        ));
        spans.push(Span::raw(" "));

        // Don't add inline progress bar here - we'll render it separately

        // Activity name
        spans.push(Span::styled(
            &self.name,
            Style::default()
                .fg(Color::White)
                .add_modifier(style_modifier),
        ));

        // Evaluation count (for evaluating activities)
        if let Some(count) = &self.evaluation_count {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("{} files", count),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(style_modifier),
            ));
        }

        // Details (machine/substituter)
        if !self.details.is_empty() {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                &self.details,
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(style_modifier),
            ));
        }

        // Current phase for builds
        if let Some(phase) = &self.current_phase {
            spans.push(Span::raw(" - "));
            spans.push(Span::styled(phase, Style::default().fg(Color::Magenta)));
        }

        // Download stats - only show if we have actual byte data
        if self.activity_type == NixActivityType::Download {
            if let (Some(downloaded), Some(speed)) = (self.bytes_downloaded, self.download_speed) {
                // Only show byte stats if we have actual download data (not just 0 bytes)
                if downloaded > 0 || self.total_bytes.is_some() {
                    spans.push(Span::raw(" - "));
                    if let Some(total) = self.total_bytes {
                        spans.push(Span::raw(format!(
                            "{}/{} @ {}",
                            crate::display::format_bytes(downloaded),
                            crate::display::format_bytes(total),
                            crate::display::format_speed(speed)
                        )));
                    } else {
                        spans.push(Span::raw(format!(
                            "{} @ {}",
                            crate::display::format_bytes(downloaded),
                            crate::display::format_speed(speed)
                        )));
                    }
                }
            }
        }

        // Calculate progress value and label
        let (progress_ratio, progress_label) = if let Some(progress) = self.progress {
            (Some(progress), None)
        } else if let Some(ref gp) = self.generic_progress {
            if gp.expected > 0 {
                let ratio = gp.done as f64 / gp.expected as f64;
                let label = if self.activity_type == NixActivityType::Download {
                    // For downloads with bytes, format them nicely
                    if gp.expected > 1000 {
                        // These are bytes, not package counts
                        format!(
                            "{}/{}",
                            crate::display::format_bytes(gp.done),
                            crate::display::format_bytes(gp.expected)
                        )
                    } else if gp.done == 0 && gp.expected == 1 {
                        "waiting".to_string()
                    } else {
                        format!("{}/{}", gp.done, gp.expected)
                    }
                } else if gp.running > 0 || gp.failed > 0 {
                    format!(
                        "{}/{} ({} running, {} failed)",
                        gp.done, gp.expected, gp.running, gp.failed
                    )
                } else {
                    format!("{}/{}", gp.done, gp.expected)
                };
                (Some(ratio), Some(label))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        // If we have progress, we need to allocate space for the gauge
        if let Some(ratio) = progress_ratio {
            // Calculate available space
            let elapsed_len = elapsed_str.len() as u16;
            let gauge_width = 20u16; // Width for the progress bar
            let label_width = progress_label
                .as_ref()
                .map(|l| l.len() as u16 + 1)
                .unwrap_or(0); // +1 for spacing

            // Render main content with limited width
            let content_width = width.saturating_sub(gauge_width + label_width + elapsed_len + 4);
            let line = Line::from(spans);
            let paragraph = Paragraph::new(line);
            paragraph.render(Rect::new(x, y, content_width, 1), buf);

            // Render progress gauge
            let gauge_x = x + content_width + 1;

            // Use blue for downloads to match the "Downloading" text
            let (filled_color, unfilled_color) = if self.activity_type == NixActivityType::Download
            {
                (Color::Blue, Color::DarkGray)
            } else {
                (Color::Cyan, Color::DarkGray)
            };

            let gauge = LineGauge::default()
                .ratio(ratio)
                .label(progress_label.unwrap_or_default())
                .filled_style(Style::default().fg(filled_color))
                .unfilled_style(Style::default().fg(unfilled_color))
                .line_set(ratatui::symbols::line::THICK);

            gauge.render(Rect::new(gauge_x, y, gauge_width + label_width, 1), buf);

            // Render timing at the end
            let timing_x = x + width.saturating_sub(elapsed_len);
            let timing_line = Line::from(Span::styled(
                elapsed_str,
                Style::default().fg(Color::DarkGray),
            ));
            let timing_paragraph = Paragraph::new(timing_line);
            timing_paragraph.render(Rect::new(timing_x, y, elapsed_len, 1), buf);
        } else {
            // No progress bar - render normally
            let line = Line::from(spans);
            let paragraph = Paragraph::new(line);
            paragraph.render(Rect::new(x, y, width, 1), buf);

            // Render the timing at a fixed position from the right
            let elapsed_len = elapsed_str.len() as u16;
            let timing_x = x + width.saturating_sub(elapsed_len + 2);
            let timing_line = Line::from(Span::styled(
                elapsed_str,
                Style::default().fg(Color::DarkGray),
            ));
            let timing_paragraph = Paragraph::new(timing_line);
            timing_paragraph.render(Rect::new(timing_x, y, elapsed_len, 1), buf);
        }
    }
}

/// Summary statistics
#[derive(Default)]
struct SummaryStats {
    builds_running: usize,
    builds_completed: usize,
    builds_failed: usize,
    downloads_running: usize,
    downloads_completed: usize,
    downloads_failed: usize,
    queries_running: usize,
    queries_completed: usize,
    queries_failed: usize,
    total_download_speed: f64,
}
