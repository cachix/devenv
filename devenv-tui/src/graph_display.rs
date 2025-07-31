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

        // Add active derivations only
        for deriv in &derivations {
            if deriv.state == NixActivityState::Active {
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

        // Get ALL operations (not just active ones) to include completed activities
        let all_operations = self.state.get_all_operations();

        for op in &all_operations {
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

        // Layout: Always have 2 sections - activities and summary
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Min(1),    // Graph area uses remaining space
                Constraint::Length(1), // Summary area (always visible, 1 line)
            ])
            .split(area);

        // Render activities in the main area (chunks[0])
        // Store the visible height for scroll calculations
        let activities_area = chunks[0];
        self.activities_visible_height = activities_area.height;

        let graph_widget = GraphWidget {
            activities: &activities,
            selected_index: self.selected_index,
            scroll_offset: self.scroll_offset,
            state: &self.state,
        };
        graph_widget.render(activities_area, buf);

        // Always render summary at the bottom (chunks[1])
        let summary = self.calculate_summary();
        let summary_widget = SummaryWidget {
            stats: summary,
            has_selection: self.selected_index.is_some(),
        };
        summary_widget.render(chunks[1], buf);
    }
}

/// Widget for the main activity graph
struct GraphWidget<'a> {
    activities: &'a [ActivityInfo],
    selected_index: Option<usize>,
    scroll_offset: usize,
    state: &'a Arc<TuiState>,
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
            let mut current_y = inner.y;
            let mut activity_index = self.scroll_offset;

            while current_y < inner.bottom() && activity_index < self.activities.len() {
                let activity = &self.activities[activity_index];
                let is_selected = self.selected_index == Some(activity_index);

                // Render the activity line
                let lines_used =
                    activity.render_line(inner.x, current_y, inner.width, is_selected, buf);
                current_y += lines_used;

                // If this is a selected build, show logs below it
                if is_selected && activity.activity_type == NixActivityType::Build {
                    if let Some(activity_id) = activity.activity_id {
                        let build_logs = self.state.get_build_logs(activity_id);

                        // Show up to 10 recent log lines
                        let max_log_lines = 10;
                        let log_lines_to_show = build_logs.len().min(max_log_lines);
                        // Always use fixed height to ensure proper clearing
                        let log_height = max_log_lines;

                        // Check if we have enough space
                        if current_y + log_height as u16 <= inner.bottom() {
                            // Calculate same indent as the activity "Building" text
                            // Activities start with 2 spaces for selection indicator, then depth indent
                            let activity_indent = 2 + if activity.depth > 0 {
                                (activity.depth * 2) + 3
                            } else {
                                0
                            };

                            // Position logs at same horizontal position as "Building" text
                            let log_x = inner.x + activity_indent as u16;
                            let log_width = inner.width.saturating_sub(activity_indent as u16);
                            let log_area =
                                Rect::new(log_x, current_y, log_width, log_height as u16);

                            // We'll render the border and content manually for better control

                            // Prepare log content with padding to fill the area
                            let mut log_content = Vec::new();

                            if log_lines_to_show > 0 {
                                // Get recent log lines
                                let start_idx = build_logs.len().saturating_sub(log_lines_to_show);
                                for log in &build_logs[start_idx..] {
                                    log_content.push(Line::from(Span::styled(
                                        log.as_str(),
                                        Style::default().fg(Color::Gray),
                                    )));
                                }
                            } else {
                                // Show waiting message
                                log_content.push(Line::from(Span::styled(
                                    "Waiting for build output...",
                                    Style::default()
                                        .fg(Color::DarkGray)
                                        .add_modifier(Modifier::ITALIC),
                                )));
                            }

                            // Pad with empty lines to fill the block
                            while log_content.len() < max_log_lines {
                                log_content.push(Line::from(""));
                            }

                            // Render logs manually to ensure proper clearing
                            for (i, line) in log_content.iter().enumerate() {
                                let y = log_area.y + i as u16;
                                if y < log_area.bottom() {
                                    // Clear the entire line first
                                    for x in 0..log_area.width {
                                        if let Some(cell) = buf.cell_mut((log_area.x + x, y)) {
                                            cell.set_char(' ').set_style(Style::default());
                                        }
                                    }

                                    // Draw the left border
                                    if let Some(cell) = buf.cell_mut((log_area.x, y)) {
                                        cell.set_char('│')
                                            .set_style(Style::default().fg(Color::DarkGray));
                                    }

                                    // Render the log line with padding
                                    let text_x = log_area.x + 2; // 1 for border, 1 for padding
                                    let text_width = log_area.width.saturating_sub(2);

                                    // Get the text content from the line
                                    let text_content = line
                                        .spans
                                        .iter()
                                        .map(|span| span.content.as_ref())
                                        .collect::<String>();

                                    // Render each character explicitly to ensure spaces are rendered
                                    for (i, ch) in text_content.chars().enumerate() {
                                        if i < text_width as usize {
                                            if let Some(cell) = buf.cell_mut((text_x + i as u16, y))
                                            {
                                                cell.set_char(ch)
                                                    .set_style(Style::default().fg(Color::Gray));
                                            }
                                        }
                                    }
                                }
                            }

                            current_y += max_log_lines as u16;
                        }
                    }
                }

                // If this is a download with progress, show progress bar below it
                if activity.activity_type == NixActivityType::Download {
                    // Calculate progress
                    let progress_ratio = if let Some(progress) = activity.progress {
                        Some(progress)
                    } else if let Some(ref gp) = activity.generic_progress {
                        if gp.expected > 0 {
                            Some(gp.done as f64 / gp.expected as f64)
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if let Some(ratio) = progress_ratio {
                        if current_y < inner.bottom() {
                            // Draw tree structure with proper indentation
                            let tree_indent = "  ".repeat(activity.depth + 1); // +1 for child indentation
                            let tree_branch = "└─";

                            let mut progress_spans = vec![];

                            // Add selection indicator space
                            progress_spans.push(Span::raw("  "));

                            // Add tree structure
                            progress_spans.push(Span::raw(&tree_indent));
                            progress_spans.push(Span::styled(
                                tree_branch,
                                Style::default().fg(Color::DarkGray),
                            ));

                            // Add percentage (no space after tree)
                            let percentage = format!("{:3.0}% ", ratio * 100.0);
                            progress_spans
                                .push(Span::styled(percentage, Style::default().fg(Color::White)));

                            // Calculate position after prefix and percentage
                            let prefix_width = 2 + tree_indent.len() + tree_branch.len() + 4; // 4 for "50% "

                            // Render the prefix with percentage
                            let prefix_line = Line::from(progress_spans);
                            let prefix_paragraph = Paragraph::new(prefix_line);
                            prefix_paragraph
                                .render(Rect::new(inner.x, current_y, prefix_width as u16, 1), buf);

                            // Calculate bytes text width to reserve space
                            let bytes_text = if let Some(ref gp) = activity.generic_progress {
                                // For downloads, generic progress contains byte counts
                                if gp.expected > 1000 {
                                    // Likely bytes, not package counts
                                    format!(
                                        "{} / {}",
                                        crate::display::format_bytes(gp.done),
                                        crate::display::format_bytes(gp.expected)
                                    )
                                } else if let (Some(downloaded), Some(total)) =
                                    (activity.bytes_downloaded, activity.total_bytes)
                                {
                                    format!(
                                        "{} / {}",
                                        crate::display::format_bytes(downloaded),
                                        crate::display::format_bytes(total)
                                    )
                                } else {
                                    String::new()
                                }
                            } else if let (Some(downloaded), Some(total)) =
                                (activity.bytes_downloaded, activity.total_bytes)
                            {
                                format!(
                                    "{} / {}",
                                    crate::display::format_bytes(downloaded),
                                    crate::display::format_bytes(total)
                                )
                            } else {
                                String::new()
                            };
                            let bytes_text_width = bytes_text.len() as u16;

                            // Calculate exact position for bytes text (2 spaces from right)
                            let bytes_x =
                                inner.x + inner.width.saturating_sub(bytes_text_width + 2);

                            // Calculate gauge width to fill space between percentage and bytes
                            let gauge_start = inner.x + prefix_width as u16;
                            let gauge_end = bytes_x.saturating_sub(1); // 1 space before bytes
                            let gauge_width = gauge_end.saturating_sub(gauge_start);

                            // Manually render the progress bar
                            let filled_width = (gauge_width as f64 * ratio) as u16;
                            let unfilled_width = gauge_width.saturating_sub(filled_width);

                            // Draw filled portion
                            if filled_width > 0 {
                                let filled_bar = "━".repeat(filled_width as usize);
                                let filled_span =
                                    Span::styled(filled_bar, Style::default().fg(Color::Blue));
                                let filled_line = Line::from(filled_span);
                                let filled_paragraph = Paragraph::new(filled_line);
                                filled_paragraph.render(
                                    Rect::new(gauge_start, current_y, filled_width, 1),
                                    buf,
                                );
                            }

                            // Draw unfilled portion
                            if unfilled_width > 0 {
                                let unfilled_bar = "━".repeat(unfilled_width as usize);
                                let unfilled_span = Span::styled(
                                    unfilled_bar,
                                    Style::default().fg(Color::DarkGray),
                                );
                                let unfilled_line = Line::from(unfilled_span);
                                let unfilled_paragraph = Paragraph::new(unfilled_line);
                                unfilled_paragraph.render(
                                    Rect::new(
                                        gauge_start + filled_width,
                                        current_y,
                                        unfilled_width,
                                        1,
                                    ),
                                    buf,
                                );
                            }

                            // Add download info aligned to the right (same as timing)
                            if !bytes_text.is_empty() {
                                let info_line = Line::from(Span::styled(
                                    bytes_text,
                                    Style::default().fg(Color::DarkGray),
                                ));
                                let info_paragraph = Paragraph::new(info_line);
                                info_paragraph.render(
                                    Rect::new(bytes_x, current_y, bytes_text_width, 1),
                                    buf,
                                );
                            }

                            current_y += 1;
                        }
                    }
                }

                activity_index += 1;
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
        // Split the area into left (summary) and right (help) sections
        let split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(50),    // Summary stats
                Constraint::Length(40), // Help text
            ])
            .split(area);

        // Left side: Summary stats in one line
        let mut spans = vec![];
        let mut has_content = false;

        // Add queries if any activity
        if self.stats.queries_running > 0 || self.stats.queries_completed > 0 {
            if has_content {
                spans.push(Span::styled("  │  ", Style::default().fg(Color::DarkGray)));
            }
            spans.extend(self.format_summary(
                "Queries",
                self.stats.queries_running,
                self.stats.queries_completed,
            ));
            has_content = true;
        }

        // Add downloads if any activity
        if self.stats.downloads_running > 0 || self.stats.downloads_completed > 0 {
            if has_content {
                spans.push(Span::styled("  │  ", Style::default().fg(Color::DarkGray)));
            }
            spans.extend(self.format_summary(
                "Downloads",
                self.stats.downloads_running,
                self.stats.downloads_completed,
            ));
            has_content = true;
        }

        // Add builds if any activity
        if self.stats.builds_running > 0 || self.stats.builds_completed > 0 {
            if has_content {
                spans.push(Span::styled("  │  ", Style::default().fg(Color::DarkGray)));
            }
            spans.extend(self.format_summary(
                "Builds",
                self.stats.builds_running,
                self.stats.builds_completed,
            ));
            has_content = true;
        }

        // If no activity at all, show a simple message
        if !has_content {
            spans.push(Span::styled(
                "No activity",
                Style::default().fg(Color::DarkGray),
            ));
        }

        let summary_line = Line::from(spans);
        let summary_paragraph = Paragraph::new(vec![summary_line]);
        summary_paragraph.render(split[0], buf);

        // Right side: Help text
        if split.len() > 1 && split[1].width > 10 {
            let help_lines = if self.has_selection {
                // When showing build logs, only show the deselect option
                vec![Line::from(vec![
                    Span::styled("Esc", Style::default().fg(Color::Yellow)),
                    Span::raw(" deselect"),
                ])]
            } else if self.stats.builds_running > 0 {
                vec![Line::from(vec![
                    Span::styled("↑↓", Style::default().fg(Color::Yellow)),
                    Span::raw(" show build logs"),
                ])]
            } else {
                vec![]
            };

            let help_paragraph = Paragraph::new(help_lines).alignment(Alignment::Right);
            help_paragraph.render(split[1], buf);
        }
    }
}

impl SummaryWidget {
    fn format_summary(&self, category: &str, running: usize, done: usize) -> Vec<Span> {
        let mut spans = vec![];

        // Category name
        spans.push(Span::raw(format!("{}: ", category)));

        let mut parts = vec![];

        // Only show running if > 0
        if running > 0 {
            parts.push(vec![
                Span::styled(format!("{}", running), Style::default().fg(Color::Blue)),
                Span::raw(" running"),
            ]);
        }

        // Only show done if > 0
        if done > 0 {
            parts.push(vec![
                Span::styled(format!("{}", done), Style::default().fg(Color::Green)),
                Span::raw(" done"),
            ]);
        }

        // Join parts with comma
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(", "));
            }
            spans.extend(part.clone());
        }

        // If both are 0 (shouldn't happen due to outer check, but just in case)
        if parts.is_empty() {
            spans.push(Span::styled("0", Style::default().fg(Color::DarkGray)));
        }

        spans
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
            bytes_downloaded: None,
            total_bytes: None,
            depth,
            evaluation_count: None,
        }
    }

    /// Render a single line for this activity
    fn render_line(&self, x: u16, y: u16, width: u16, is_selected: bool, buf: &mut Buffer) -> u16 {
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
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                phase,
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(style_modifier),
            ));
        }

        // Download stats are now shown as a child progress bar, not inline

        // Calculate progress value and label (but don't show inline for downloads)
        let (progress_ratio, progress_label) = if self.activity_type == NixActivityType::Download {
            // Downloads show progress as a child, not inline
            (None, None)
        } else if let Some(progress) = self.progress {
            (Some(progress), None)
        } else if let Some(ref gp) = self.generic_progress {
            if gp.expected > 0 {
                let ratio = gp.done as f64 / gp.expected as f64;
                let label = if gp.running > 0 || gp.failed > 0 {
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

        1 // Return height used (1 line for now, will update for logs)
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
}
