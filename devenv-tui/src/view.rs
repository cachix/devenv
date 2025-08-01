use crate::{
    model::{ActivityInfo, ActivitySummary, Model},
    NixActivityState, NixActivityType,
};
use ansi_to_tui::IntoText;
use human_repr::{HumanCount, HumanDuration, HumanThroughput};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{LineGauge, Paragraph},
    Frame,
};
use std::time::Duration;

/// Spinner animation frames (matching current CLI)
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// View function following The Elm Architecture
/// Renders the UI based on the current model state
pub fn view(model: &Model, frame: &mut Frame) {
    let area = frame.area();

    // Don't clear - we want to preserve terminal output above

    // Use the graph display style from the original implementation
    render_graph_display(model, frame, area);
}

/// Render the graph display (matching the old GraphDisplay)
fn render_graph_display(model: &Model, frame: &mut Frame, area: Rect) {
    let active_activities = model.get_active_activities();

    // If no active activities, clear the area
    if active_activities.is_empty() {
        // Clear the TUI area when no activities are running
        frame.render_widget(ratatui::widgets::Clear, area);
        return;
    }

    // Layout: Always have 2 sections - activities and summary (matching original)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // Graph area uses remaining space
            Constraint::Length(1), // Summary area (always visible, 1 line at bottom)
        ])
        .split(area);

    // Render the main activity graph with inline logs (no side panel)
    render_activity_graph(&active_activities, model, frame, chunks[0]);

    // Always render summary at the bottom (chunks[1])
    let summary = model.calculate_summary();
    render_summary_line(
        &summary,
        model.ui.selected_activity_index.is_some(),
        frame,
        chunks[1],
    );
}

/// Render the activity graph
fn render_activity_graph(
    activities: &[ActivityInfo],
    model: &Model,
    frame: &mut Frame,
    area: Rect,
) {
    // Add a small margin on the left (matching original)
    let inner = Rect {
        x: area.x + 1,
        y: area.y,
        width: area.width.saturating_sub(1),
        height: area.height,
    };

    if activities.is_empty() {
        // Show a message when no activities are running
        let no_activities_msg = Line::from(vec![Span::styled(
            "No active operations",
            Style::default().fg(Color::DarkGray),
        )]);
        let paragraph = Paragraph::new(vec![no_activities_msg]).alignment(Alignment::Center);
        frame.render_widget(paragraph, inner);
        return;
    }

    // Render each activity
    let mut y = inner.y;
    for (idx, activity) in activities.iter().enumerate() {
        if y >= inner.y + inner.height {
            break; // No more room
        }

        let is_selected = model.ui.selected_activity_index == Some(idx);
        render_activity_line(
            model,
            activity,
            inner.x,
            y,
            inner.width,
            is_selected,
            frame,
            &model.ui.spinner_frame,
        );

        y += 1;

        // If this is a selected build, show logs inline below it (matching original)
        if is_selected && activity.activity_type == NixActivityType::Build {
            if let Some(activity_id) = activity.activity_id {
                if let Some(logs) = model.build_logs.get(&activity_id) {
                    // Calculate available space for logs
                    let remaining_height = (inner.y + inner.height).saturating_sub(y);

                    // Check if this is the last activity or if there's space after
                    let is_last_activity = idx == activities.len() - 1;
                    let next_activities_count = activities.len().saturating_sub(idx + 1);

                    // Use more space if available, but leave room for other activities
                    let max_log_lines = if is_last_activity {
                        // If it's the last activity, use all remaining space
                        remaining_height.min(20) as usize
                    } else {
                        // Otherwise, use up to 10 lines but leave at least 1 line per remaining activity
                        let space_for_logs =
                            remaining_height.saturating_sub(next_activities_count as u16);
                        space_for_logs.min(10) as usize
                    };

                    let log_lines_to_show = logs.len().min(max_log_lines);

                    // Check if we have enough space for at least some logs
                    if max_log_lines > 0 && y < inner.y + inner.height {
                        // Calculate same indent as the activity "Building" text
                        let activity_indent = 2 + if activity.depth > 0 {
                            (activity.depth * 2) + 3
                        } else {
                            0
                        };

                        // Position logs at same horizontal position as "Building" text
                        let log_x = inner.x + activity_indent as u16;
                        let log_width = inner.width.saturating_sub(activity_indent as u16 + 2);

                        // Show recent log lines
                        if log_lines_to_show > 0 {
                            let start_idx = logs.len().saturating_sub(max_log_lines);
                            let mut lines_rendered = 0;
                            for (i, log) in logs.iter().skip(start_idx).enumerate() {
                                if lines_rendered >= log_lines_to_show
                                    || y + lines_rendered as u16 >= inner.y + inner.height
                                {
                                    break;
                                }

                                // Render border and log line
                                let border_span =
                                    Span::styled("│ ", Style::default().fg(Color::DarkGray));

                                // Try to parse ANSI codes and render the log line
                                match log.into_text() {
                                    Ok(text) => {
                                        // Get the first line's spans and prepend the border
                                        if let Some(line) = text.lines.into_iter().next() {
                                            let mut log_spans = vec![border_span];
                                            log_spans.extend(line.spans);
                                            let log_line = Line::from(log_spans);
                                            let log_paragraph = Paragraph::new(log_line);
                                            frame.render_widget(
                                                log_paragraph,
                                                Rect {
                                                    x: log_x,
                                                    y: y + lines_rendered as u16,
                                                    width: log_width,
                                                    height: 1,
                                                },
                                            );
                                        }
                                    }
                                    Err(_) => {
                                        // Fallback to plain text
                                        let log_line = Line::from(vec![
                                            border_span,
                                            Span::styled(log, Style::default().fg(Color::Gray)),
                                        ]);
                                        let log_paragraph = Paragraph::new(log_line);
                                        frame.render_widget(
                                            log_paragraph,
                                            Rect {
                                                x: log_x,
                                                y: y + lines_rendered as u16,
                                                width: log_width,
                                                height: 1,
                                            },
                                        );
                                    }
                                }
                                lines_rendered += 1;
                            }
                        } else {
                            // Show waiting message
                            let waiting_line = Line::from(vec![
                                Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                                Span::styled(
                                    "Waiting for build output...",
                                    Style::default()
                                        .fg(Color::DarkGray)
                                        .add_modifier(Modifier::ITALIC),
                                ),
                            ]);
                            let waiting_paragraph = Paragraph::new(waiting_line);
                            frame.render_widget(
                                waiting_paragraph,
                                Rect {
                                    x: log_x,
                                    y,
                                    width: log_width,
                                    height: 1,
                                },
                            );
                        }

                        // Advance y past the actual log lines shown
                        y += if log_lines_to_show > 0 {
                            log_lines_to_show as u16
                        } else {
                            1 // For the "waiting" message
                        };
                    }
                }
            }
        }
    }
}

/// Render a single activity line (matching old graph_display format)
fn render_activity_line(
    model: &Model,
    activity: &ActivityInfo,
    x: u16,
    y: u16,
    width: u16,
    is_selected: bool,
    frame: &mut Frame,
    spinner_frame: &usize,
) {
    let spinner = SPINNER_FRAMES[*spinner_frame];
    let elapsed = activity.start_time.elapsed();
    let elapsed_str = format_duration(elapsed);

    // Check if this is a download with progress
    let has_progress = activity.activity_type == NixActivityType::Download
        && matches!(activity.state, NixActivityState::Active)
        && activity.bytes_downloaded.is_some()
        && activity.total_bytes.is_some();

    if has_progress {
        // Render with progress bar
        render_activity_with_progress(
            model,
            activity,
            x,
            y,
            width,
            is_selected,
            spinner,
            &elapsed_str,
            frame,
        );
    } else {
        // Build the line based on type and state
        let line = match &activity.state {
            NixActivityState::Active => {
                render_active_activity(model, activity, is_selected, spinner, &elapsed_str)
            }
            NixActivityState::Completed { success, duration } => {
                render_completed_activity(activity, is_selected, *success, *duration)
            }
        };

        let paragraph = Paragraph::new(line);
        frame.render_widget(
            paragraph,
            Rect {
                x,
                y,
                width,
                height: 1,
            },
        );
    }
}

/// Render activity with inline progress bar (for downloads)
fn render_activity_with_progress(
    _model: &Model,
    activity: &ActivityInfo,
    x: u16,
    y: u16,
    width: u16,
    is_selected: bool,
    spinner: &str,
    elapsed_str: &str,
    frame: &mut Frame,
) {
    let mut spans = vec![];

    // Selection indicator
    if is_selected {
        spans.push(Span::styled(
            "▶ ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(Span::raw("  "));
    }

    // Add indentation for hierarchy
    if activity.depth > 0 {
        spans.push(Span::raw("  ".repeat(activity.depth)));
        spans.push(Span::styled("└─ ", Style::default().fg(Color::DarkGray)));
    }

    // Spinner and activity type
    spans.push(Span::styled(
        spinner.to_string(),
        Style::default().fg(Color::Blue),
    ));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        "Downloading",
        Style::default().fg(Color::Blue),
    ));
    spans.push(Span::raw(" "));
    spans.push(Span::raw(activity.short_name.clone()));

    // Calculate space for progress bar
    let prefix_len = spans.iter().map(|s| s.content.len()).sum::<usize>() as u16;
    let elapsed_len = elapsed_str.len() as u16 + 3; // [elapsed]
    let gauge_width = 20u16;
    let percentage_width = 5u16; // "100%"

    // Calculate available width for the main text
    let text_width = width.saturating_sub(gauge_width + percentage_width + elapsed_len + 4);

    // Render the prefix
    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(
        paragraph,
        Rect {
            x,
            y,
            width: text_width.min(prefix_len),
            height: 1,
        },
    );

    // Render progress gauge
    if let (Some(downloaded), Some(total)) = (activity.bytes_downloaded, activity.total_bytes) {
        let ratio = downloaded as f64 / total as f64;
        let percentage = (ratio * 100.0) as u16;

        // Progress bar position
        let gauge_x = x + text_width.min(prefix_len) + 1;

        let gauge = LineGauge::default()
            .ratio(ratio)
            .label(format!("{}%", percentage))
            .filled_style(Style::default().fg(Color::Blue))
            .unfilled_style(Style::default().fg(Color::DarkGray))
            .filled_symbol(ratatui::symbols::line::THICK.horizontal)
            .unfilled_symbol(ratatui::symbols::line::THICK.horizontal);

        frame.render_widget(
            gauge,
            Rect {
                x: gauge_x,
                y,
                width: gauge_width + percentage_width,
                height: 1,
            },
        );

        // Render timing at the end
        let timing_x = x + width.saturating_sub(elapsed_len);
        let timing_line = Line::from(Span::styled(
            format!("[{}]", elapsed_str),
            Style::default().fg(Color::DarkGray),
        ));
        let timing_paragraph = Paragraph::new(timing_line);
        frame.render_widget(
            timing_paragraph,
            Rect {
                x: timing_x,
                y,
                width: elapsed_len,
                height: 1,
            },
        );
    }
}

/// Render an active activity line
fn render_active_activity(
    model: &Model,
    activity: &ActivityInfo,
    is_selected: bool,
    spinner: &str,
    elapsed_str: &str,
) -> Line<'static> {
    let mut spans = vec![];

    // Selection indicator (matching original)
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

    // Add indentation for hierarchy
    if activity.depth > 0 {
        spans.push(Span::raw("  ".repeat(activity.depth)));
        spans.push(Span::styled("└─ ", Style::default().fg(Color::DarkGray)));
    }

    // Spinner and activity type word (all in-progress activities use blue)
    let (type_word, color) = match activity.activity_type {
        NixActivityType::Build => ("Building", Color::Blue),
        NixActivityType::Download => ("Downloading", Color::Blue),
        NixActivityType::Query => ("Querying", Color::Blue),
        NixActivityType::FetchTree => ("Fetching", Color::Blue),
        NixActivityType::Evaluating => ("Evaluating", Color::Blue),
        _ => ("Processing", Color::Blue),
    };

    spans.push(Span::styled(
        spinner.to_string(),
        Style::default().fg(color),
    ));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        type_word.to_string(),
        Style::default().fg(color),
    ));
    spans.push(Span::raw(" "));
    spans.push(Span::raw(activity.short_name.clone()));

    // Add evaluation count for evaluating activities
    if activity.activity_type == NixActivityType::Evaluating {
        if let Some(count) = &activity.evaluation_count {
            spans.push(Span::raw(" ("));
            spans.push(Span::styled(
                count.clone(),
                Style::default().fg(Color::Cyan),
            ));
            spans.push(Span::raw(" evaluated)"));
        }
    }

    // Add machine info for builds
    if activity.activity_type == NixActivityType::Build {
        if let Some(activity_id) = activity.activity_id {
            // Look for matching derivation with machine info
            for derivation in model.nix_derivations.values() {
                if derivation.activity_id == activity_id {
                    if let Some(machine) = &derivation.machine {
                        spans.push(Span::raw(" on "));
                        spans.push(Span::styled(
                            machine.clone(),
                            Style::default().fg(Color::Yellow),
                        ));
                    }
                    break;
                }
            }
        }
    }

    // Add phase info for builds
    if let Some(phase) = &activity.current_phase {
        spans.push(Span::raw(" - "));
        spans.push(Span::styled(
            phase.clone(),
            Style::default().fg(Color::DarkGray),
        ));
    }

    // Add elapsed time at the end
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        format!("[{}]", elapsed_str),
        Style::default().fg(Color::DarkGray),
    ));

    Line::from(spans)
}

/// Render a completed activity line
fn render_completed_activity(
    activity: &ActivityInfo,
    is_selected: bool,
    success: bool,
    duration: Duration,
) -> Line<'static> {
    let mut spans = vec![];

    // Selection indicator
    if is_selected {
        spans.push(Span::styled(
            "▶ ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(Span::raw("  "));
    }

    // Add indentation for hierarchy
    if activity.depth > 0 {
        spans.push(Span::raw("  ".repeat(activity.depth)));
        spans.push(Span::styled("└─ ", Style::default().fg(Color::DarkGray)));
    }

    let (symbol, color) = if success {
        ("✓", Color::Green)
    } else {
        ("✗", Color::Red)
    };

    spans.push(Span::styled(symbol, Style::default().fg(color)));
    spans.push(Span::raw(" "));

    // Activity type prefix
    match activity.activity_type {
        NixActivityType::Build => spans.push(Span::raw("Built ")),
        NixActivityType::Download => spans.push(Span::raw("Downloaded ")),
        NixActivityType::Query => spans.push(Span::raw("Queried ")),
        NixActivityType::FetchTree => spans.push(Span::raw("Fetched ")),
        NixActivityType::Evaluating => spans.push(Span::raw("Evaluated ")),
        _ => {}
    }

    spans.push(Span::raw(activity.short_name.clone()));
    spans.push(Span::raw(" in "));
    spans.push(Span::styled(
        format_duration(duration),
        Style::default().fg(Color::DarkGray),
    ));

    Line::from(spans)
}

/// Render the summary line at the bottom (exactly matching original)
fn render_summary_line(
    summary: &ActivitySummary,
    has_selection: bool,
    frame: &mut Frame,
    area: Rect,
) {
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
    if summary.active_queries > 0 {
        if has_content {
            spans.push(Span::styled("  │  ", Style::default().fg(Color::DarkGray)));
        }
        spans.extend(format_summary_section("Queries", summary.active_queries, 0));
        has_content = true;
    }

    // Add downloads if any activity
    if summary.active_downloads > 0 {
        if has_content {
            spans.push(Span::styled("  │  ", Style::default().fg(Color::DarkGray)));
        }
        spans.extend(format_summary_section(
            "Downloads",
            summary.active_downloads,
            0,
        ));
        has_content = true;
    }

    // Add builds if any activity
    if summary.active_builds > 0 || summary.completed_builds > 0 {
        if has_content {
            spans.push(Span::styled("  │  ", Style::default().fg(Color::DarkGray)));
        }
        spans.extend(format_summary_section(
            "Builds",
            summary.active_builds,
            summary.completed_builds,
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
    frame.render_widget(summary_paragraph, split[0]);

    // Right side: Help text
    if split.len() > 1 && split[1].width > 10 {
        let help_lines = if has_selection {
            vec![Line::from(vec![
                Span::styled("Esc", Style::default().fg(Color::Yellow)),
                Span::raw(" hide build log"),
            ])]
        } else if summary.active_builds > 0 {
            vec![Line::from(vec![
                Span::styled("↑↓", Style::default().fg(Color::Yellow)),
                Span::raw(" show build logs"),
            ])]
        } else {
            vec![]
        };

        let help_paragraph = Paragraph::new(help_lines).alignment(Alignment::Right);
        frame.render_widget(help_paragraph, split[1]);
    }
}

/// Format a summary section (matching original format exactly)
fn format_summary_section(category: &str, running: usize, done: usize) -> Vec<Span<'static>> {
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

    // If all are 0 (shouldn't happen due to outer check, but just in case)
    if parts.is_empty() {
        spans.push(Span::styled("0", Style::default().fg(Color::DarkGray)));
    }

    spans
}

/// Format duration using human-repr for consistency
pub fn format_duration(duration: Duration) -> String {
    duration.human_duration().to_string()
}

/// Format bytes using human-repr
pub fn format_bytes(bytes: u64) -> String {
    (bytes as f64).human_count_bytes().to_string()
}

/// Format download speed using human-repr
pub fn format_speed(bytes_per_sec: f64) -> String {
    bytes_per_sec.human_throughput_bytes().to_string()
}
