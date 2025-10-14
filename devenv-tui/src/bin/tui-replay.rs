use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use devenv_eval_cache::internal_log::{ActivityType, Field, InternalLog};
use devenv_tui::{OperationId, init_tui};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use std::sync::{Arc, Mutex};
use tokio::time::sleep;
use tokio_shutdown::Shutdown;
use tracing::{debug_span, info, warn};
use tracing::span::EnteredSpan;

/// Simple replay processor that emits tracing events for Nix logs
struct NixLogReplayProcessor {
    current_operation_id: Arc<Mutex<Option<OperationId>>>,
    active_spans: Arc<Mutex<HashMap<u64, EnteredSpan>>>,
}

impl NixLogReplayProcessor {
    fn new() -> Self {
        Self {
            current_operation_id: Arc::new(Mutex::new(None)),
            active_spans: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn set_current_operation(&self, operation_id: OperationId) {
        if let Ok(mut current) = self.current_operation_id.lock() {
            *current = Some(operation_id);
        }
    }

    fn process_internal_log(&self, log: InternalLog) {
        let current_op_id = self
            .current_operation_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone());

        if let Some(operation_id) = current_op_id {
            match log {
                InternalLog::Start {
                    id,
                    typ,
                    text,
                    fields,
                    ..
                } => {
                    self.handle_activity_start(operation_id, id, typ, text, fields);
                }
                InternalLog::Stop { id } => {
                    self.handle_activity_stop(id, true);
                }
                InternalLog::Result { id, typ, fields } => {
                    self.handle_activity_result(id, typ, fields);
                }
                _ => {
                    // For other log types, emit a basic tracing event
                    info!(devenv.log = true, "Nix: {:?}", log);
                }
            }
        }
    }

    fn handle_activity_start(
        &self,
        operation_id: OperationId,
        activity_id: u64,
        activity_type: ActivityType,
        text: String,
        fields: Vec<Field>,
    ) {
        match activity_type {
            ActivityType::Build => {
                let derivation_path = fields
                    .first()
                    .and_then(|f| match f {
                        Field::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| text.clone());

                let machine = fields.get(1).and_then(|f| match f {
                    Field::String(s) => Some(s.clone()),
                    _ => None,
                });

                let derivation_name = extract_derivation_name(&derivation_path);

                let span = debug_span!(
                    target: "devenv.nix.build",
                    "nix_derivation_start",
                    devenv.ui.message = %derivation_name,
                    devenv.ui.type = "build",
                    devenv.ui.id = %operation_id,
                    activity_id = activity_id,
                    derivation_path = %derivation_path,
                    derivation_name = %derivation_name,
                    machine = ?machine
                );

                let entered_span = span.entered();
                if let Ok(mut spans) = self.active_spans.lock() {
                    spans.insert(activity_id, entered_span);
                }
            }
            ActivityType::CopyPath => {
                if let (Some(Field::String(store_path)), Some(Field::String(substituter))) =
                    (fields.first(), fields.get(1))
                {
                    let package_name = extract_package_name(store_path);

                    let span = debug_span!(
                        target: "devenv.nix.download",
                        "nix_download_start",
                        devenv.ui.message = %package_name,
                        devenv.ui.type = "download",
                        devenv.ui.id = %operation_id,
                        activity_id = activity_id,
                        store_path = %store_path,
                        package_name = %package_name,
                        substituter = %substituter
                    );

                    let entered_span = span.entered();
                    if let Ok(mut spans) = self.active_spans.lock() {
                        spans.insert(activity_id, entered_span);
                    }
                }
            }
            ActivityType::QueryPathInfo => {
                if let (Some(Field::String(store_path)), Some(Field::String(substituter))) =
                    (fields.first(), fields.get(1))
                {
                    let package_name = extract_package_name(store_path);

                    let span = debug_span!(
                        target: "devenv.nix.query",
                        "nix_query_start",
                        devenv.ui.message = %package_name,
                        devenv.ui.type = "download",
                        devenv.ui.detail = "query",
                        devenv.ui.id = %operation_id,
                        activity_id = activity_id,
                        store_path = %store_path,
                        package_name = %package_name,
                        substituter = %substituter
                    );

                    let entered_span = span.entered();
                    if let Ok(mut spans) = self.active_spans.lock() {
                        spans.insert(activity_id, entered_span);
                    }
                }
            }
            ActivityType::FetchTree => {
                let span = debug_span!(
                    target: "devenv.nix.fetch",
                    "fetch_tree_start",
                    devenv.ui.message = %text,
                    devenv.ui.type = "download",
                    devenv.ui.detail = "fetch",
                    devenv.ui.id = %operation_id,
                    activity_id = activity_id,
                    message = %text
                );

                let entered_span = span.entered();
                if let Ok(mut spans) = self.active_spans.lock() {
                    spans.insert(activity_id, entered_span);
                }
            }
            _ => {
                // For other activity types, emit a debug event
                tracing::debug!("Unhandled Nix activity type: {:?}", activity_type);
            }
        }
    }

    fn handle_activity_stop(&self, activity_id: u64, _success: bool) {
        // Remove the span from active spans - dropping it will close the span
        if let Ok(mut spans) = self.active_spans.lock() {
            spans.remove(&activity_id);
        }
    }

    fn handle_activity_result(
        &self,
        activity_id: u64,
        result_type: devenv_eval_cache::internal_log::ResultType,
        fields: Vec<Field>,
    ) {
        use devenv_eval_cache::internal_log::{Field, ResultType};

        match result_type {
            ResultType::Progress => {
                if fields.len() >= 2
                    && let (Some(Field::Int(downloaded)), total_opt) =
                        (fields.first(), fields.get(1))
                {
                    let total_bytes = match total_opt {
                        Some(Field::Int(total)) => Some(*total),
                        _ => None,
                    };

                    // Emit progress event (not a span)
                    tracing::event!(
                        target: "devenv.nix.download",
                        tracing::Level::DEBUG,
                        devenv.ui.message = "download progress",
                        devenv.ui.id = "progress",
                        activity_id = activity_id,
                        bytes_downloaded = *downloaded,
                        total_bytes = ?total_bytes,
                        "Download progress: {} / {:?} bytes",
                        downloaded,
                        total_bytes
                    );
                }
            }
            ResultType::SetPhase => {
                if let Some(Field::String(phase)) = fields.first() {
                    // Emit phase change event
                    tracing::event!(
                        target: "devenv.nix.build",
                        tracing::Level::DEBUG,
                        devenv.ui.message = "build phase",
                        devenv.ui.id = "phase",
                        activity_id = activity_id,
                        phase = phase.as_str(),
                        "Build phase: {}", phase
                    );
                }
            }
            ResultType::BuildLogLine => {
                if let Some(Field::String(log_line)) = fields.first() {
                    // Emit build log event (not a span)
                    tracing::event!(
                        target: "devenv.nix.build",
                        tracing::Level::INFO,
                        devenv.ui.message = "build log",
                        activity_id = activity_id,
                        line = %log_line,
                        "Build log: {}", log_line
                    );
                }
            }
            _ => {
                tracing::debug!("Unhandled Nix result type: {:?}", result_type);
            }
        }
    }
}

/// Extract a human-readable derivation name from a derivation path
fn extract_derivation_name(derivation_path: &str) -> String {
    // Remove .drv suffix if present
    let path = derivation_path
        .strip_suffix(".drv")
        .unwrap_or(derivation_path);

    // Extract the name part after the hash
    if let Some(dash_pos) = path.rfind('-')
        && let Some(slash_pos) = path[..dash_pos].rfind('/')
    {
        return path[slash_pos + 1..].to_string();
    }

    // Fallback: just take the filename
    path.split('/').next_back().unwrap_or(path).to_string()
}

/// Extract a human-readable package name from a store path
fn extract_package_name(store_path: &str) -> String {
    // Extract the name part after the hash (format: /nix/store/hash-name)
    if let Some(dash_pos) = store_path.rfind('-')
        && let Some(slash_pos) = store_path[..dash_pos].rfind('/')
    {
        return store_path[slash_pos + 1..].to_string();
    }

    // Fallback: just take the filename
    store_path
        .split('/')
        .next_back()
        .unwrap_or(store_path)
        .to_string()
}

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
    let shutdown = Shutdown::new();

    // Run replay and then wait for shutdown
    run_replay(shutdown.clone()).await?;

    // Keep running until Ctrl+C
    eprintln!("Replay complete. Press Ctrl+C to exit.");
    shutdown.wait_for_shutdown().await;

    Ok(())
}

async fn run_replay(shutdown: Arc<Shutdown>) -> Result<()> {
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

    // Initialize TUI with proper shutdown coordination
    let tui_handle = init_tui();

    // Get model before moving the layer
    let model = tui_handle.model();

    // Initialize basic tracing to support the new architecture
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry().with(tui_handle.layer).init();

    // Create a Nix log replay processor that emits tracing events
    let nix_processor = NixLogReplayProcessor::new();

    // Start TUI in background
    let tui_shutdown = shutdown.clone();
    let tui_handle = tokio::spawn(async move {
        match devenv_tui::app::run_app(model, tui_shutdown).await {
            Ok(_) => eprintln!("TUI exited normally"),
            Err(e) => eprintln!("TUI error: {}", e),
        }
    });

    // Create main operation
    let main_op_id = OperationId::new("replay");

    // Start replay operation via tracing - using a span so the TUI layer can capture it
    let replay_span = debug_span!(
        "replay_operation",
        devenv.ui.message = format!("Replaying {} log entries", entries.len()),
        devenv.ui.id = %main_op_id,
    );
    let _guard = replay_span.enter();

    // Set current operation for Nix processor
    nix_processor.set_current_operation(main_op_id.clone());

    // Replay log entries with timing
    let start_time = Instant::now();
    let first_timestamp = entries[0].timestamp;

    for (idx, entry) in entries.iter().enumerate() {
        // Calculate delay from first entry
        let time_offset = entry.timestamp.signed_duration_since(first_timestamp);
        let target_elapsed = Duration::from_millis(time_offset.num_milliseconds() as u64);

        // Wait until we reach the target time, with shutdown support
        let current_elapsed = start_time.elapsed();
        if target_elapsed > current_elapsed {
            let sleep_duration = target_elapsed - current_elapsed;
            tokio::select! {
                _ = sleep(sleep_duration) => {
                    // Continue with replay
                }
                _ = shutdown.wait_for_shutdown() => {
                    // Shutdown requested, cleanup and exit
                    warn!("Replay interrupted by shutdown");

                    // Give TUI a moment to display the message
                    sleep(Duration::from_millis(100)).await;

                    return Ok(());
                }
            }
        }

        // Process the log entry based on source
        match entry.source.as_str() {
            "@nix" => {
                // Try to parse as Nix internal log
                if let Ok(internal_log) = serde_json::from_str::<InternalLog>(&entry.content) {
                    nix_processor.process_internal_log(internal_log);
                } else {
                    // Log as regular message
                    info!(target: "devenv.nix", "{}", entry.content);
                }
            }
            _ => {
                // Log as regular message
                info!(target: "devenv.system", "{} {}", entry.source, entry.content);
            }
        }

        // Show progress
        if idx % 100 == 0 {
            let progress = ((idx + 1) as f64 / entries.len() as f64) * 100.0;
            info!("Replay progress: {:.1}%", progress);
        }
    }

    // Keep the span open and let activities continue to be displayed
    // Don't drop _guard here - keep the replay operation active
    info!("Replay finished. Activities are still visible in the TUI.");

    // Check if TUI is still running
    if tui_handle.is_finished() {
        eprintln!("Warning: TUI task has already exited!");
    }

    Ok(())
}
