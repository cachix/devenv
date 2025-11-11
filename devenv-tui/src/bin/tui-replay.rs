use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use devenv_tui::TuiHandle;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tokio_shutdown::Shutdown;
use tracing::span::EnteredSpan;
use tracing::{error, info, info_span, warn};

/// JSON trace event from tracing-subscriber's JSON formatter
#[derive(Debug, Deserialize, Clone)]
struct TraceEvent {
    timestamp: DateTime<Utc>,
    #[serde(default)]
    fields: HashMap<String, serde_json::Value>,
    #[serde(default)]
    span: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    spans: Vec<HashMap<String, serde_json::Value>>,
}

/// Helper to extract typed values from JSON fields
struct FieldExtractor<'a> {
    event_fields: &'a HashMap<String, serde_json::Value>,
    span_fields: &'a HashMap<String, serde_json::Value>,
}

impl<'a> FieldExtractor<'a> {
    fn new(
        event_fields: &'a HashMap<String, serde_json::Value>,
        span_fields: &'a HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            event_fields,
            span_fields,
        }
    }

    fn str_field(&self, key: &str) -> String {
        self.span_fields
            .get(key)
            .or_else(|| self.event_fields.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }

    fn u64_field(&self, key: &str) -> u64 {
        self.span_fields
            .get(key)
            .or_else(|| self.event_fields.get(key))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    }

    fn span_name(&self) -> String {
        self.span_fields
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    }

    fn is_new_span(&self) -> bool {
        self.event_fields.get("message").and_then(|v| v.as_str()) == Some("new")
    }
}

/// Replays JSON trace events by recreating spans and events
struct TraceReplayer {
    active_spans: Mutex<Vec<(String, EnteredSpan)>>, // Track (span_name, entered_span)
    last_depth: Mutex<usize>,
}

impl TraceReplayer {
    fn new() -> Self {
        Self {
            active_spans: Mutex::new(Vec::new()),
            last_depth: Mutex::new(0),
        }
    }

    fn replay(&self, event: &TraceEvent) {
        let span_fields = event.span.clone().unwrap_or_default();
        let fields = FieldExtractor::new(&event.fields, &span_fields);
        let target_depth = event.spans.len();

        // Determine the span name at this level
        let current_span_name = span_fields
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Only adjust spans if depth changed or if this is a new event
        if fields.is_new_span() {
            self.adjust_spans(&current_span_name, &span_fields, target_depth);
        } else {
            // For non-new events, still close spans if depth decreased
            if let Ok(mut spans) = self.active_spans.lock() {
                while spans.len() > target_depth {
                    spans.pop();
                }
                if let Ok(mut last_depth) = self.last_depth.lock() {
                    *last_depth = target_depth;
                }
            }
            self.emit_event(&fields);
        }
    }

    /// Create or close spans based on depth changes
    fn adjust_spans(
        &self,
        current_span_name: &str,
        span_fields: &HashMap<String, serde_json::Value>,
        target_depth: usize,
    ) {
        if let Ok(mut spans) = self.active_spans.lock() {
            if let Ok(mut last_depth) = self.last_depth.lock() {
                let prev_depth = *last_depth;

                // Close spans if we exited them
                while spans.len() > target_depth {
                    spans.pop();
                }

                // Only create a new span if depth increased
                if target_depth > prev_depth && target_depth > spans.len() {
                    let span = self.create_span(current_span_name, span_fields);
                    spans.push((current_span_name.to_string(), span.entered()));
                }

                *last_depth = target_depth;
            }
        }
    }

    fn create_span(
        &self,
        _name: &str,
        span_fields: &HashMap<String, serde_json::Value>,
    ) -> tracing::Span {
        let empty = HashMap::new();
        let fields = FieldExtractor::new(&empty, span_fields);
        let operation_type = fields.str_field("devenv.ui.operation.type");
        let op_name = fields.str_field("devenv.ui.operation.name");
        let op_short_name = fields.str_field("devenv.ui.operation.short_name");

        match operation_type.as_str() {
            "build" => {
                let derivation = fields.str_field("devenv.ui.details.derivation");
                info_span!(
                    "nix_build",
                    "devenv.ui.operation.type" = "build",
                    "devenv.ui.operation.name" = op_name.as_str(),
                    "devenv.ui.operation.short_name" = op_short_name.as_str(),
                    "devenv.ui.details.derivation" = derivation.as_str(),
                )
            }
            "download" => {
                let store_path = fields.str_field("devenv.ui.details.store_path");
                let substituter = fields.str_field("devenv.ui.details.substituter");
                info_span!(
                    "nix_download",
                    "devenv.ui.operation.type" = "download",
                    "devenv.ui.operation.name" = op_name.as_str(),
                    "devenv.ui.operation.short_name" = op_short_name.as_str(),
                    "devenv.ui.details.store_path" = store_path.as_str(),
                    "devenv.ui.details.substituter" = substituter.as_str(),
                )
            }
            "query" => {
                let store_path = fields.str_field("devenv.ui.details.store_path");
                let substituter = fields.str_field("devenv.ui.details.substituter");
                info_span!(
                    "nix_query",
                    "devenv.ui.operation.type" = "query",
                    "devenv.ui.operation.name" = op_name.as_str(),
                    "devenv.ui.operation.short_name" = op_short_name.as_str(),
                    "devenv.ui.details.store_path" = store_path.as_str(),
                    "devenv.ui.details.substituter" = substituter.as_str(),
                )
            }
            "fetch_tree" => {
                info_span!(
                    "fetch_tree",
                    "devenv.ui.operation.type" = "fetch_tree",
                    "devenv.ui.operation.name" = op_name.as_str(),
                    "devenv.ui.operation.short_name" = op_short_name.as_str(),
                )
            }
            "evaluate" => {
                info_span!(
                    "nix_evaluate",
                    "devenv.ui.operation.type" = "evaluate",
                    "devenv.ui.operation.name" = op_name.as_str(),
                    "devenv.ui.operation.short_name" = op_short_name.as_str(),
                )
            }
            _ => info_span!("operation"),
        }
    }

    fn emit_event(&self, fields: &FieldExtractor) {
        let message = fields.str_field("message");
        let progress_type = fields.str_field("devenv.ui.progress.type");
        let progress_current = fields.u64_field("devenv.ui.progress.current");
        let progress_total = fields.u64_field("devenv.ui.progress.total");
        let progress_rate = fields.u64_field("devenv.ui.progress.rate");
        let status = fields.str_field("devenv.ui.status");

        if !message.is_empty() {
            if !progress_type.is_empty() {
                info!(
                    "devenv.ui.progress.type" = progress_type.as_str(),
                    "devenv.ui.progress.current" = progress_current,
                    "devenv.ui.progress.total" = progress_total,
                    "devenv.ui.progress.rate" = progress_rate,
                    "devenv.ui.status" = status.as_str(),
                    "{}",
                    message
                );
            } else if !status.is_empty() {
                info!("devenv.ui.status" = status.as_str(), "{}", message);
            } else {
                info!("{}", message);
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize TUI
    let tui_handle = TuiHandle::init();

    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry()
        .with(tui_handle.layer())
        .with(tracing_subscriber::fmt::layer())
        .init();

    let shutdown = Shutdown::new();
    run_replay(tui_handle, shutdown.clone()).await?;

    info!("Replay complete. Press Ctrl+C to exit.");
    shutdown.wait_for_shutdown().await;
    Ok(())
}

async fn run_replay(tui_handle: TuiHandle, shutdown: Arc<Shutdown>) -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        info!(
            r"Usage: {binary} <trace-file.json>
        Generate a trace file using:
          devenv --trace-export-file=trace.json <command>
        Or with JSON log format:
          devenv --log-format=tracing-json <command> > trace.json",
            binary = args[0]
        );
        std::process::exit(1);
    }

    let trace_path = PathBuf::from(&args[1]);
    let file = File::open(&trace_path)
        .with_context(|| format!("Failed to open trace file: {}", trace_path.display()))?;

    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    // Parse all trace entries
    for (line_num, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("Failed to read line {}", line_num + 1))?;
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<TraceEvent>(&line) {
            Ok(event) => entries.push(event),
            Err(e) => {
                error!(
                    "Failed to parse JSON on line {line_num}: {e}",
                    line_num = line_num + 1
                );
                error!("Line: {line}");
            }
        }
    }

    if entries.is_empty() {
        error!("No valid trace entries found");
        return Ok(());
    }

    info!("Loaded {} trace events", entries.len());

    let replayer = TraceReplayer::new();

    // Start TUI in background
    let tui_task = tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            match devenv_tui::app::run_app(tui_handle.model(), shutdown).await {
                Ok(_) => info!("TUI exited normally"),
                Err(e) => error!("TUI error: {e}"),
            }
        }
    });

    // Replay trace entries with timing
    let start_time = Instant::now();
    let first_timestamp = entries[0].timestamp;

    for (idx, event) in entries.iter().enumerate() {
        // Calculate delay from first entry
        let time_offset = event.timestamp.signed_duration_since(first_timestamp);
        let target_elapsed = Duration::from_millis(time_offset.num_milliseconds().max(0) as u64);

        // Wait until we reach the target time
        let current_elapsed = start_time.elapsed();
        if target_elapsed > current_elapsed {
            let sleep_duration = target_elapsed - current_elapsed;
            tokio::select! {
                _ = sleep(sleep_duration) => {}
                _ = shutdown.wait_for_shutdown() => {
                    warn!("Replay interrupted by shutdown");
                    sleep(Duration::from_millis(100)).await;
                    return Ok(());
                }
            }
        }

        replayer.replay(event);
    }

    info!("Replay finished. Activities are still visible in the TUI.");

    if tui_task.is_finished() {
        warn!("Warning: TUI task has already exited!");
    }

    Ok(())
}
