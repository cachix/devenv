use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use devenv_activity::{
    ActivityEvent, ActivityKind, ActivityOutcome, LogLevel, ProgressState, ProgressUnit, Timestamp,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_shutdown::Shutdown;
use tracing::{info, warn};

/// Raw trace event from JSON log format
#[derive(Debug, Deserialize, Clone)]
struct RawTraceEvent {
    timestamp: DateTime<Utc>,
    #[serde(default)]
    target: String,
    #[serde(default)]
    fields: HashMap<String, serde_json::Value>,
}

fn get_string(fields: &HashMap<String, serde_json::Value>, key: &str) -> Option<String> {
    fields.get(key).and_then(|v| match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

fn get_u64(fields: &HashMap<String, serde_json::Value>, key: &str) -> Option<u64> {
    fields.get(key).and_then(|v| v.as_u64())
}

fn datetime_to_timestamp(dt: DateTime<Utc>) -> Timestamp {
    let duration = dt.signed_duration_since(DateTime::UNIX_EPOCH);
    let system_time = SystemTime::UNIX_EPOCH
        + Duration::from_secs(duration.num_seconds() as u64)
        + Duration::from_nanos(duration.num_nanoseconds().unwrap_or(0) as u64 % 1_000_000_000);
    Timestamp::from(system_time)
}

struct TraceStream {
    lines: std::io::Lines<BufReader<File>>,
    line_num: usize,
    first_timestamp: Option<DateTime<Utc>>,
}

impl TraceStream {
    fn new(file: File) -> Self {
        Self {
            lines: BufReader::new(file).lines(),
            line_num: 0,
            first_timestamp: None,
        }
    }

    fn next_event(&mut self) -> Result<Option<RawTraceEvent>> {
        loop {
            self.line_num += 1;

            match self.lines.next() {
                Some(Ok(line)) => {
                    if line.trim().is_empty() {
                        continue;
                    }

                    match serde_json::from_str::<RawTraceEvent>(&line) {
                        Ok(event) => {
                            if self.first_timestamp.is_none() {
                                self.first_timestamp = Some(event.timestamp);
                            }
                            return Ok(Some(event));
                        }
                        Err(e) => {
                            warn!(
                                "Failed to parse JSON on line {}: {}. Skipping...",
                                self.line_num, e
                            );
                            continue;
                        }
                    }
                }
                Some(Err(e)) => {
                    return Err(anyhow::anyhow!(
                        "Failed to read line {}: {}",
                        self.line_num,
                        e
                    ));
                }
                None => return Ok(None),
            }
        }
    }
}

/// Converts trace events to ActivityEvents and sends them via channel
struct EventProcessor {
    tx: mpsc::UnboundedSender<ActivityEvent>,
}

impl EventProcessor {
    fn new(tx: mpsc::UnboundedSender<ActivityEvent>) -> Self {
        Self { tx }
    }

    fn send(&self, event: ActivityEvent) {
        let _ = self.tx.send(event);
    }

    fn process(&mut self, raw: &RawTraceEvent) {
        if raw.target != "devenv::activity" {
            return;
        }

        let fields = &raw.fields;
        let timestamp = datetime_to_timestamp(raw.timestamp);

        if let Some(event_type) = get_string(fields, "event_type") {
            match event_type.as_str() {
                "start" => self.handle_start(fields, timestamp),
                "complete" => self.handle_complete(fields, timestamp),
                _ => {}
            }
            return;
        }

        if get_string(fields, "progress_kind").is_some() {
            self.handle_progress(fields, timestamp);
            return;
        }

        if let Some(phase) = get_string(fields, "phase") {
            if let Some(id) = get_u64(fields, "activity_id") {
                self.send(ActivityEvent::Phase {
                    id,
                    phase,
                    timestamp,
                });
            }
            return;
        }

        if let Some(line) = get_string(fields, "log_line") {
            if let Some(id) = get_u64(fields, "activity_id") {
                let is_error = fields
                    .get("log_is_error")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.send(ActivityEvent::Log {
                    id,
                    line,
                    is_error,
                    timestamp,
                });
            }
            return;
        }

        if let Some(text) = get_string(fields, "message_text") {
            let level = match get_string(fields, "message_level").as_deref() {
                Some("error") => LogLevel::Error,
                Some("warn") => LogLevel::Warn,
                Some("info") => LogLevel::Info,
                Some("debug") => LogLevel::Debug,
                Some("trace") => LogLevel::Trace,
                _ => LogLevel::Info,
            };
            self.send(ActivityEvent::Message {
                level,
                text,
                timestamp,
            });
        }
    }

    fn handle_start(
        &mut self,
        fields: &HashMap<String, serde_json::Value>,
        timestamp: Timestamp,
    ) {
        let Some(id) = get_u64(fields, "activity_id") else {
            return;
        };

        let kind = match get_string(fields, "kind").as_deref() {
            Some("build") => ActivityKind::Build,
            Some("fetch") => ActivityKind::Fetch,
            Some("evaluate") => ActivityKind::Evaluate,
            Some("task") => ActivityKind::Task,
            Some("command") => ActivityKind::Command,
            _ => ActivityKind::Operation,
        };

        let name = get_string(fields, "name").unwrap_or_else(|| "Unknown".to_string());

        let parent = get_string(fields, "parent").and_then(|p| {
            if p == "None" {
                None
            } else {
                p.parse::<u64>().ok()
            }
        });

        let detail = get_string(fields, "detail").and_then(|d| {
            if d == "None" {
                None
            } else {
                Some(d)
            }
        });

        self.send(ActivityEvent::Start {
            id,
            kind,
            name,
            parent,
            detail,
            timestamp,
        });
    }

    fn handle_complete(
        &mut self,
        fields: &HashMap<String, serde_json::Value>,
        timestamp: Timestamp,
    ) {
        let Some(id) = get_u64(fields, "activity_id") else {
            return;
        };

        let outcome = match get_string(fields, "outcome").as_deref() {
            Some("success") => ActivityOutcome::Success,
            Some("failed") => ActivityOutcome::Failed,
            Some("cancelled") => ActivityOutcome::Cancelled,
            _ => ActivityOutcome::Success,
        };

        self.send(ActivityEvent::Complete {
            id,
            outcome,
            timestamp,
        });
    }

    fn handle_progress(
        &mut self,
        fields: &HashMap<String, serde_json::Value>,
        timestamp: Timestamp,
    ) {
        let Some(id) = get_u64(fields, "activity_id") else {
            return;
        };

        let progress_kind = get_string(fields, "progress_kind");
        let current = get_u64(fields, "progress_current").unwrap_or(0);

        let unit = match get_string(fields, "progress_unit").as_deref() {
            Some("bytes") => Some(ProgressUnit::Bytes),
            Some("files") => Some(ProgressUnit::Files),
            Some("items") => Some(ProgressUnit::Items),
            _ => None,
        };

        let progress = match progress_kind.as_deref() {
            Some("determinate") => {
                let total = get_u64(fields, "progress_total").unwrap_or(0);
                ProgressState::Determinate {
                    current,
                    total,
                    unit,
                }
            }
            _ => ProgressState::Indeterminate { current, unit },
        };

        self.send(ActivityEvent::Progress {
            id,
            progress,
            timestamp,
        });
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry().init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!(
            r"Usage: {} <trace-file.jsonl>

Generate a trace file using:
  devenv --trace-export-file=trace.jsonl <command>

Or with JSON log format:
  devenv --log-format=tracing-json <command> > trace.jsonl 2>&1

Then replay with:
  {} trace.jsonl",
            args[0], args[0]
        );
        std::process::exit(1);
    }

    let trace_path = PathBuf::from(&args[1]);
    let file = File::open(&trace_path)
        .with_context(|| format!("Failed to open trace file: {}", trace_path.display()))?;

    let (tx, rx) = mpsc::unbounded_channel();
    let shutdown = Shutdown::new();

    let tui_task = tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            match devenv_tui::app::run_app(rx, shutdown).await {
                Ok(_) => info!("TUI exited normally"),
                Err(e) => warn!("TUI error: {e}"),
            }
        }
    });

    let mut stream = TraceStream::new(file);
    let first_event = stream
        .next_event()?
        .with_context(|| "No valid trace entries found")?;

    let first_timestamp = first_event.timestamp;
    info!("Starting trace replay from: {}", trace_path.display());

    let start_time = Instant::now();
    let mut processor = EventProcessor::new(tx);
    let mut event_count = 0;

    processor.process(&first_event);

    while let Some(event) = stream.next_event()? {
        event_count += 1;
        if event_count % 100 == 0 {
            info!("Processed {} events", event_count);
        }

        let time_offset = event.timestamp.signed_duration_since(first_timestamp);
        let target_elapsed = Duration::from_millis(time_offset.num_milliseconds().max(0) as u64);

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

        processor.process(&event);
    }

    info!(
        "Replay finished. Processed {} total events.",
        event_count + 1
    );
    info!("Press Ctrl+C to exit the TUI");

    tokio::select! {
        _ = tui_task => {
            info!("TUI task completed");
        }
        _ = shutdown.wait_for_shutdown() => {
            info!("Shutdown requested");
        }
    }

    Ok(())
}
