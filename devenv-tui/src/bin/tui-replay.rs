use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use clap::Parser;
use devenv_activity::{
    ActivityEvent, ActivityKind, ActivityOutcome, LogLevel, ProgressState, ProgressUnit, Timestamp,
};
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};
use tokio::signal;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_shutdown::Shutdown;
use tracing::{info, warn};

#[derive(Parser)]
#[command(about = "Replay devenv trace files with TUI visualization")]
struct Args {
    /// Path to the trace file (JSONL format)
    trace_file: PathBuf,

    /// Replay speed multiplier (e.g., 2.0 for 2x speed, 0.5 for half speed)
    #[arg(long, short, default_value = "1.0")]
    speed: f64,
}

/// Raw trace event from JSON log format (tracing output)
#[derive(Debug, Deserialize, Clone)]
struct RawTraceEvent {
    timestamp: DateTime<Utc>,
    #[serde(default)]
    target: String,
    #[serde(default)]
    fields: TraceFields,
}

/// Typed fields from tracing output
#[derive(Debug, Deserialize, Clone, Default)]
struct TraceFields {
    activity_id: Option<u64>,
    event_type: Option<String>,
    kind: Option<String>,
    name: Option<String>,
    parent: Option<u64>,
    detail: Option<String>,
    outcome: Option<String>,
    progress_kind: Option<String>,
    progress_current: Option<u64>,
    progress_total: Option<u64>,
    progress_unit: Option<String>,
    phase: Option<String>,
    log_line: Option<String>,
    log_is_error: Option<bool>,
    message_level: Option<String>,
    message_text: Option<String>,
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

        if let Some(ref event_type) = fields.event_type {
            match event_type.as_str() {
                "start" => self.handle_start(fields, timestamp),
                "complete" => self.handle_complete(fields, timestamp),
                _ => {}
            }
            return;
        }

        if fields.progress_kind.is_some() {
            self.handle_progress(fields, timestamp);
            return;
        }

        if let Some(ref phase) = fields.phase {
            if let Some(id) = fields.activity_id {
                self.send(ActivityEvent::Phase {
                    id,
                    phase: phase.clone(),
                    timestamp,
                });
            }
            return;
        }

        if let Some(ref line) = fields.log_line {
            if let Some(id) = fields.activity_id {
                let is_error = fields.log_is_error.unwrap_or(false);
                self.send(ActivityEvent::Log {
                    id,
                    line: line.clone(),
                    is_error,
                    timestamp,
                });
            }
            return;
        }

        if let Some(ref text) = fields.message_text {
            let level = match fields.message_level.as_deref() {
                Some("error") => LogLevel::Error,
                Some("warn") => LogLevel::Warn,
                Some("info") => LogLevel::Info,
                Some("debug") => LogLevel::Debug,
                Some("trace") => LogLevel::Trace,
                _ => LogLevel::Info,
            };
            self.send(ActivityEvent::Message {
                level,
                text: text.clone(),
                timestamp,
            });
        }
    }

    fn handle_start(&mut self, fields: &TraceFields, timestamp: Timestamp) {
        let Some(id) = fields.activity_id else {
            return;
        };

        let kind = match fields.kind.as_deref() {
            Some("build") => ActivityKind::Build,
            Some("fetch") => ActivityKind::Fetch,
            Some("evaluate") => ActivityKind::Evaluate,
            Some("task") => ActivityKind::Task,
            Some("command") => ActivityKind::Command,
            _ => ActivityKind::Operation,
        };

        let name = fields.name.clone().unwrap_or_else(|| "Unknown".to_string());

        self.send(ActivityEvent::Start {
            id,
            kind,
            name,
            parent: fields.parent,
            detail: fields.detail.clone(),
            timestamp,
        });
    }

    fn handle_complete(&mut self, fields: &TraceFields, timestamp: Timestamp) {
        let Some(id) = fields.activity_id else {
            return;
        };

        let outcome = match fields.outcome.as_deref() {
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

    fn handle_progress(&mut self, fields: &TraceFields, timestamp: Timestamp) {
        let Some(id) = fields.activity_id else {
            return;
        };

        let current = fields.progress_current.unwrap_or(0);

        let unit = match fields.progress_unit.as_deref() {
            Some("bytes") => Some(ProgressUnit::Bytes),
            Some("files") => Some(ProgressUnit::Files),
            Some("items") => Some(ProgressUnit::Items),
            _ => None,
        };

        let progress = match fields.progress_kind.as_deref() {
            Some("determinate") => {
                let total = fields.progress_total.unwrap_or(0);
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

async fn ctrl_c() {
    signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
}

async fn replay_events(
    mut stream: TraceStream,
    first_event: RawTraceEvent,
    processor: &mut EventProcessor,
    speed: f64,
) -> Result<()> {
    let first_timestamp = first_event.timestamp;
    let start_time = Instant::now();
    let mut event_count = 0;

    processor.process(&first_event);

    while let Some(event) = stream.next_event()? {
        event_count += 1;
        if event_count % 100 == 0 {
            info!("Processed {} events", event_count);
        }

        let time_offset = event.timestamp.signed_duration_since(first_timestamp);
        let target_elapsed_ms = time_offset.num_milliseconds().max(0) as f64 / speed;
        let target_elapsed = Duration::from_millis(target_elapsed_ms as u64);

        let current_elapsed = start_time.elapsed();
        if target_elapsed > current_elapsed {
            let sleep_duration = target_elapsed - current_elapsed;
            sleep(sleep_duration).await;
        }

        processor.process(&event);
    }

    info!(
        "Replay finished. Processed {} total events.",
        event_count + 1
    );

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry().init();

    let args = Args::parse();

    if args.speed <= 0.0 {
        bail!("Speed must be greater than 0");
    }

    let file = File::open(&args.trace_file)
        .with_context(|| format!("Failed to open trace file: {}", args.trace_file.display()))?;

    let (tx, rx) = mpsc::unbounded_channel();
    let shutdown = Shutdown::new();

    let mut tui_task = tokio::spawn({
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

    info!("Starting trace replay from: {}", args.trace_file.display());

    let mut processor = EventProcessor::new(tx);

    // Race: replay events, TUI task, or Ctrl+C
    tokio::select! {
        result = replay_events(stream, first_event, &mut processor, args.speed) => {
            if let Err(e) = result {
                warn!("Replay error: {e}");
            }
            info!("Press Ctrl+C to exit the TUI");
            // Replay finished, wait for TUI or Ctrl+C
            tokio::select! {
                _ = &mut tui_task => {}
                _ = ctrl_c() => {}
            }
        }
        _ = &mut tui_task => {
            info!("TUI exited");
        }
        _ = ctrl_c() => {
            info!("Interrupted");
        }
    }

    // Restore terminal to normal state (disable raw mode, show cursor)
    devenv_tui::app::restore_terminal();

    Ok(())
}
