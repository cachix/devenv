use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use clap::Parser;
use devenv_activity::ActivityEvent;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use thiserror::Error;
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

/// Raw trace event as it appears in the JSONL file
#[derive(Debug, Deserialize)]
struct TraceEvent {
    target: String,
    timestamp: DateTime<Utc>,
    fields: serde_json::Value,
}

#[derive(Debug, Error)]
enum ActivityParseError {
    #[error("not a devenv activity event")]
    NotActivityEvent,
    #[error("failed to deserialize activity: {0}")]
    DeserializationError(#[from] serde_json::Error),
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

    fn next_event(&mut self) -> Result<Option<TraceEvent>> {
        loop {
            self.line_num += 1;

            match self.lines.next() {
                Some(Ok(line)) => {
                    if line.trim().is_empty() {
                        continue;
                    }

                    match serde_json::from_str::<TraceEvent>(&line) {
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

/// Deserializes a trace event into an ActivityEvent.
fn deserialize_activity(event: TraceEvent) -> Result<ActivityEvent, ActivityParseError> {
    if event.target != "devenv::activity" {
        return Err(ActivityParseError::NotActivityEvent);
    }

    // The event field contains the JSON-serialized ActivityEvent as a string
    let event_json = event
        .fields
        .get("event")
        .and_then(|v| v.as_str())
        .ok_or(ActivityParseError::NotActivityEvent)?;

    Ok(serde_json::from_str(event_json)?)
}

async fn ctrl_c() {
    signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
}

fn process_event(event: TraceEvent, tx: &mpsc::UnboundedSender<ActivityEvent>) {
    match deserialize_activity(event) {
        Ok(activity) => {
            let _ = tx.send(activity);
        }
        Err(ActivityParseError::NotActivityEvent) => {}
        Err(e) => {
            info!("Failed to parse activity event: {}", e);
        }
    }
}

async fn replay_events(
    mut stream: TraceStream,
    first_event: TraceEvent,
    tx: &mpsc::UnboundedSender<ActivityEvent>,
    speed: f64,
) -> Result<()> {
    let first_timestamp = first_event.timestamp;
    let start_time = Instant::now();
    let mut event_count = 0;

    process_event(first_event, tx);

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

        process_event(event, tx);
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

    // Race: replay events, TUI task, or Ctrl+C
    tokio::select! {
        result = replay_events(stream, first_event, &tx, args.speed) => {
            if let Err(e) = result {
                warn!("Replay error: {e}");
            }
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
