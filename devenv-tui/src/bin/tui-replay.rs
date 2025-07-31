use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use devenv_eval_cache::internal_log::InternalLog;
use devenv_tui::{
    create_nix_bridge, init_tui, DisplayMode, LogLevel, LogSource, OperationId, TuiEvent,
};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::sleep;

#[derive(Debug)]
struct LogEntry {
    timestamp: DateTime<Utc>,
    source: String,
    content: String,
}

fn parse_log_line(line: &str) -> Result<LogEntry> {
    // Find the first space after the timestamp
    let space_pos = line.find(' ').context("No space found after timestamp")?;

    let (timestamp_str, rest) = line.split_at(space_pos);
    let rest = rest.trim_start();

    // Parse ISO8601 timestamp
    let timestamp = DateTime::parse_from_rfc3339(timestamp_str)
        .context("Failed to parse ISO8601 timestamp")?
        .with_timezone(&Utc);

    // Find the source tag (e.g., @nix)
    let source_end = rest.find(' ').unwrap_or(rest.len());
    let (source, content) = rest.split_at(source_end);
    let content = content.trim_start();

    Ok(LogEntry {
        timestamp,
        source: source.to_string(),
        content: content.to_string(),
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <log-file>", args[0]);
        std::process::exit(1);
    }

    let log_path = PathBuf::from(&args[1]);
    let file = File::open(&log_path)
        .with_context(|| format!("Failed to open log file: {}", log_path.display()))?;

    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    // Parse all log entries
    for (line_num, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("Failed to read line {}", line_num + 1))?;
        if line.trim().is_empty() {
            continue;
        }

        match parse_log_line(&line) {
            Ok(entry) => entries.push(entry),
            Err(e) => eprintln!("Failed to parse line {}: {} - {}", line_num + 1, e, line),
        }
    }

    if entries.is_empty() {
        eprintln!("No valid log entries found");
        return Ok(());
    }

    // Initialize TUI
    let (_layer, state) = init_tui(DisplayMode::Ratatui);
    let nix_bridge = create_nix_bridge();

    // Create a channel to send events to TUI
    let (tx, mut rx) = mpsc::unbounded_channel::<TuiEvent>();

    // Forward events to TUI state
    let state_clone = state.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            state_clone.handle_event(event);
        }
    });

    // Create main operation
    let main_op_id = OperationId::new("replay");
    tx.send(TuiEvent::OperationStart {
        id: main_op_id.clone(),
        message: format!("Replaying {} log entries", entries.len()),
        parent: None,
        data: HashMap::new(),
    })?;

    // Set current operation for Nix bridge
    if let Some(bridge) = &nix_bridge {
        bridge.set_current_operation(main_op_id.clone());
    }

    // Replay log entries with timing
    let start_time = Instant::now();
    let first_timestamp = entries[0].timestamp;

    // Create a channel for cancellation
    let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);

    // Spawn a task to handle Ctrl-C
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        let _ = cancel_tx.send(()).await;
    });

    for (idx, entry) in entries.iter().enumerate() {
        // Calculate delay from first entry
        let time_offset = entry.timestamp.signed_duration_since(first_timestamp);
        let target_elapsed = Duration::from_millis(time_offset.num_milliseconds() as u64);

        // Wait until we reach the target time, but also listen for Ctrl-C
        let current_elapsed = start_time.elapsed();
        if target_elapsed > current_elapsed {
            let sleep_duration = target_elapsed - current_elapsed;
            tokio::select! {
                _ = sleep(sleep_duration) => {
                    // Continue with replay
                }
                _ = cancel_rx.recv() => {
                    // Ctrl-C pressed, cleanup and exit
                    tx.send(TuiEvent::LogMessage {
                        level: LogLevel::Warn,
                        message: "Replay interrupted by user".to_string(),
                        source: LogSource::System,
                        data: HashMap::new(),
                    })?;

                    // Give TUI a moment to display the message
                    sleep(Duration::from_millis(100)).await;

                    devenv_tui::cleanup_tui();
                    return Ok(());
                }
            }
        }

        // Process the log entry based on source
        match entry.source.as_str() {
            "@nix" => {
                // Try to parse as Nix internal log
                if let Ok(internal_log) = serde_json::from_str::<InternalLog>(&entry.content) {
                    if let Some(bridge) = &nix_bridge {
                        bridge.process_internal_log(internal_log);
                    }
                } else {
                    // Send as regular log message
                    tx.send(TuiEvent::LogMessage {
                        level: LogLevel::Info,
                        message: entry.content.clone(),
                        source: LogSource::Nix,
                        data: HashMap::new(),
                    })?;
                }
            }
            _ => {
                // Send as regular log message
                tx.send(TuiEvent::LogMessage {
                    level: LogLevel::Info,
                    message: format!("{} {}", entry.source, entry.content),
                    source: LogSource::System,
                    data: HashMap::new(),
                })?;
            }
        }

        // Show progress
        if idx % 100 == 0 {
            let progress = ((idx + 1) as f64 / entries.len() as f64) * 100.0;
            tx.send(TuiEvent::LogMessage {
                level: LogLevel::Info,
                message: format!("Replay progress: {:.1}%", progress),
                source: LogSource::System,
                data: HashMap::new(),
            })?;
        }
    }

    // Complete the main operation
    tx.send(TuiEvent::OperationEnd {
        id: main_op_id,
        result: devenv_tui::OperationResult::Success,
    })?;

    // Give TUI a moment to display the final state
    sleep(Duration::from_millis(100)).await;

    // Cleanup and exit
    devenv_tui::cleanup_tui();

    Ok(())
}
