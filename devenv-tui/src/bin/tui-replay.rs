use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use devenv_tui::init_tui;
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
use tracing::{info, info_span, warn};

/// JSON trace event from tracing-subscriber's JSON formatter
#[derive(Debug, Deserialize, Clone)]
struct TraceEvent {
    timestamp: DateTime<Utc>,
    #[serde(default)]
    fields: HashMap<String, serde_json::Value>,
    #[serde(default)]
    span: Option<SpanInfo>,
}

#[derive(Debug, Deserialize, Clone)]
struct SpanInfo {
    #[serde(default)]
    name: String,
    id: Option<u64>,
}

/// Helper to extract typed values from JSON fields
struct FieldExtractor<'a>(&'a HashMap<String, serde_json::Value>);

impl<'a> FieldExtractor<'a> {
    fn str_field(&self, key: &str) -> &str {
        self.0.get(key).and_then(|v| v.as_str()).unwrap_or("")
    }

    fn u64_field(&self, key: &str) -> u64 {
        self.0.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
    }

    fn event_type(&self) -> Option<&str> {
        self.0.get("event").and_then(|v| v.as_str())
    }
}

/// Replays JSON trace events by recreating spans and events
struct TraceReplayer {
    active_spans: Mutex<HashMap<u64, EnteredSpan>>,
}

impl TraceReplayer {
    fn new() -> Self {
        Self {
            active_spans: Mutex::new(HashMap::new()),
        }
    }

    fn replay(&self, event: &TraceEvent) {
        let fields = FieldExtractor(&event.fields);

        match (event.span.as_ref(), fields.event_type()) {
            (Some(span_info), Some("new")) => self.start_span(span_info, &fields),
            (Some(span_info), Some("close")) => self.end_span(span_info),
            _ => self.emit_event(&fields),
        }
    }

    fn start_span(&self, span_info: &SpanInfo, fields: &FieldExtractor) {
        let Some(span_id) = span_info.id else { return };

        let span = match fields.str_field("operation.type") {
            "build" => info_span!(
                "nix_build",
                operation.type = "build",
                operation.name = fields.str_field("operation.name"),
                operation.short_name = fields.str_field("operation.short_name"),
                operation.derivation = fields.str_field("operation.derivation"),
                nix.activity_id = fields.u64_field("nix.activity_id"),
            ),
            "download" => info_span!(
                "nix_download",
                operation.type = "download",
                operation.name = fields.str_field("operation.name"),
                operation.short_name = fields.str_field("operation.short_name"),
                operation.store_path = fields.str_field("operation.store_path"),
                operation.substituter = fields.str_field("operation.substituter"),
                nix.activity_id = fields.u64_field("nix.activity_id"),
            ),
            "query" => info_span!(
                "nix_query",
                operation.type = "query",
                operation.name = fields.str_field("operation.name"),
                operation.short_name = fields.str_field("operation.short_name"),
                operation.store_path = fields.str_field("operation.store_path"),
                operation.substituter = fields.str_field("operation.substituter"),
                nix.activity_id = fields.u64_field("nix.activity_id"),
            ),
            "fetch_tree" => info_span!(
                "fetch_tree",
                operation.type = "fetch_tree",
                operation.name = fields.str_field("operation.name"),
                operation.short_name = fields.str_field("operation.short_name"),
                nix.activity_id = fields.u64_field("nix.activity_id"),
            ),
            "evaluate" => info_span!(
                "nix_evaluate",
                operation.type = "evaluate",
                operation.name = fields.str_field("operation.name"),
                operation.short_name = fields.str_field("operation.short_name"),
                nix.activity_id = fields.u64_field("nix.activity_id"),
            ),
            _ => info_span!(
                "generic_operation",
                operation.type = fields.str_field("operation.type"),
                operation.name = fields.str_field("operation.name"),
                operation.short_name = fields.str_field("operation.short_name"),
            ),
        };

        if let Ok(mut spans) = self.active_spans.lock() {
            spans.insert(span_id, span.entered());
        }
    }

    fn end_span(&self, span_info: &SpanInfo) {
        if let (Some(span_id), Ok(mut spans)) = (span_info.id, self.active_spans.lock()) {
            spans.remove(&span_id);
        }
    }

    fn emit_event(&self, fields: &FieldExtractor) {
        let message = fields.str_field("message");
        if !message.is_empty() {
            info!("{}", message);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let shutdown = Shutdown::new();
    run_replay(shutdown.clone()).await?;

    eprintln!("Replay complete. Press Ctrl+C to exit.");
    shutdown.wait_for_shutdown().await;
    Ok(())
}

async fn run_replay(shutdown: Arc<Shutdown>) -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <trace-file.json>", args[0]);
        eprintln!();
        eprintln!("Generate a trace file using:");
        eprintln!("  devenv --trace-export-file=trace.json <command>");
        eprintln!();
        eprintln!("Or with JSON log format:");
        eprintln!("  devenv --log-format=tracing-json <command> > trace.json");
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
                eprintln!("Failed to parse JSON on line {}: {}", line_num + 1, e);
                eprintln!("Line: {}", line);
            }
        }
    }

    if entries.is_empty() {
        eprintln!("No valid trace entries found");
        return Ok(());
    }

    eprintln!("Loaded {} trace events", entries.len());

    // Initialize TUI
    let tui_handle = init_tui();
    let model = tui_handle.model();

    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry().with(tui_handle.layer).init();

    let replayer = TraceReplayer::new();

    // Start TUI in background
    let tui_task = tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            match devenv_tui::app::run_app(model, shutdown).await {
                Ok(_) => eprintln!("TUI exited normally"),
                Err(e) => eprintln!("TUI error: {}", e),
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

        // Show progress periodically
        if idx % 100 == 0 && idx > 0 {
            let progress = ((idx + 1) as f64 / entries.len() as f64) * 100.0;
            eprintln!("Replay progress: {:.1}%", progress);
        }
    }

    eprintln!("Replay finished. Activities are still visible in the TUI.");

    if tui_task.is_finished() {
        eprintln!("Warning: TUI task has already exited!");
    }

    Ok(())
}
