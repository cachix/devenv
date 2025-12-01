use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use clap::Parser;
use devenv_activity::{
    ActivityEvent, ActivityOutcome, Build, Command, Evaluate, Fetch, FetchKind, LogLevel, Message,
    Operation, Task, Timestamp,
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
    activity_type: Option<String>,
    fetch_kind: Option<String>,
    name: Option<String>,
    parent: Option<u64>,
    derivation_path: Option<String>,
    url: Option<String>,
    detail: Option<String>,
    command: Option<String>,
    outcome: Option<String>,
    progress_done: Option<u64>,
    progress_expected: Option<u64>,
    progress_current: Option<u64>,
    progress_total: Option<u64>,
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

        // Handle progress events (Build/Task use done/expected, Fetch uses current/total)
        if fields.progress_done.is_some() || fields.progress_current.is_some() {
            self.handle_progress(fields, timestamp);
            return;
        }

        // Handle phase events (Build only)
        if let Some(ref phase) = fields.phase {
            if let Some(id) = fields.activity_id {
                self.send(ActivityEvent::Build(Build::Phase {
                    id,
                    phase: phase.clone(),
                    timestamp,
                }));
            }
            return;
        }

        // Handle log events
        if let Some(ref line) = fields.log_line {
            if let Some(id) = fields.activity_id {
                self.handle_log(id, line.clone(), fields.log_is_error.unwrap_or(false), fields.activity_type.as_deref(), timestamp);
            }
            return;
        }

        // Handle message events
        if let Some(ref text) = fields.message_text {
            let level = match fields.message_level.as_deref() {
                Some("error") => LogLevel::Error,
                Some("warn") => LogLevel::Warn,
                Some("info") => LogLevel::Info,
                Some("debug") => LogLevel::Debug,
                Some("trace") => LogLevel::Trace,
                _ => LogLevel::Info,
            };
            self.send(ActivityEvent::Message(Message {
                level,
                text: text.clone(),
                timestamp,
            }));
        }
    }

    fn handle_start(&mut self, fields: &TraceFields, timestamp: Timestamp) {
        let Some(id) = fields.activity_id else {
            return;
        };

        let name = fields.name.clone().unwrap_or_else(|| "Unknown".to_string());
        let parent = fields.parent;

        let event = match fields.activity_type.as_deref() {
            Some("build") => ActivityEvent::Build(Build::Start {
                id,
                name,
                parent,
                derivation_path: fields.derivation_path.clone(),
                timestamp,
            }),
            Some("fetch") => {
                let kind = match fields.fetch_kind.as_deref() {
                    Some("download") => FetchKind::Download,
                    Some("query") => FetchKind::Query,
                    Some("tree") => FetchKind::Tree,
                    _ => FetchKind::Download,
                };
                ActivityEvent::Fetch(Fetch::Start {
                    id,
                    kind,
                    name,
                    parent,
                    url: fields.url.clone(),
                    timestamp,
                })
            }
            Some("evaluate") => ActivityEvent::Evaluate(Evaluate::Start {
                id,
                name,
                parent,
                timestamp,
            }),
            Some("task") => ActivityEvent::Task(Task::Start {
                id,
                name,
                parent,
                detail: fields.detail.clone(),
                timestamp,
            }),
            Some("command") => ActivityEvent::Command(Command::Start {
                id,
                name,
                parent,
                command: fields.command.clone(),
                timestamp,
            }),
            _ => ActivityEvent::Operation(Operation::Start {
                id,
                name,
                parent,
                detail: fields.detail.clone(),
                timestamp,
            }),
        };

        self.send(event);
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

        // We need the activity type to emit the correct Complete event.
        // In replay, we might not have it tracked, so default to Operation.
        let event = match fields.activity_type.as_deref() {
            Some("build") => ActivityEvent::Build(Build::Complete { id, outcome, timestamp }),
            Some("fetch") => ActivityEvent::Fetch(Fetch::Complete { id, outcome, timestamp }),
            Some("evaluate") => ActivityEvent::Evaluate(Evaluate::Complete { id, outcome, timestamp }),
            Some("task") => ActivityEvent::Task(Task::Complete { id, outcome, timestamp }),
            Some("command") => ActivityEvent::Command(Command::Complete { id, outcome, timestamp }),
            _ => ActivityEvent::Operation(Operation::Complete { id, outcome, timestamp }),
        };

        self.send(event);
    }

    fn handle_progress(&mut self, fields: &TraceFields, timestamp: Timestamp) {
        let Some(id) = fields.activity_id else {
            return;
        };

        // Build/Task progress uses done/expected
        if let (Some(done), Some(expected)) = (fields.progress_done, fields.progress_expected) {
            // Default to Build progress if activity_type is not specified
            let event = match fields.activity_type.as_deref() {
                Some("task") => ActivityEvent::Task(Task::Progress {
                    id,
                    done,
                    expected,
                    timestamp,
                }),
                _ => ActivityEvent::Build(Build::Progress {
                    id,
                    done,
                    expected,
                    timestamp,
                }),
            };
            self.send(event);
            return;
        }

        // Fetch progress uses current/total
        if let Some(current) = fields.progress_current {
            self.send(ActivityEvent::Fetch(Fetch::Progress {
                id,
                current,
                total: fields.progress_total,
                timestamp,
            }));
        }
    }

    fn handle_log(&mut self, id: u64, line: String, is_error: bool, activity_type: Option<&str>, timestamp: Timestamp) {
        let event = match activity_type {
            Some("build") => ActivityEvent::Build(Build::Log { id, line, is_error, timestamp }),
            Some("evaluate") => ActivityEvent::Evaluate(Evaluate::Log { id, line, timestamp }),
            Some("task") => ActivityEvent::Task(Task::Log { id, line, is_error, timestamp }),
            Some("command") => ActivityEvent::Command(Command::Log { id, line, is_error, timestamp }),
            _ => ActivityEvent::Build(Build::Log { id, line, is_error, timestamp }),
        };
        self.send(event);
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
