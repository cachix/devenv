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
fn deserialize_activity(mut event: TraceEvent) -> Result<ActivityEvent, ActivityParseError> {
    if event.target != "devenv::activity" {
        return Err(ActivityParseError::NotActivityEvent);
    }

    let event_value = event
        .fields
        .get_mut("event")
        .map(serde_json::Value::take)
        .ok_or(ActivityParseError::NotActivityEvent)?;

    Ok(serde_json::from_value(event_value)?)
}

fn process_event(tx: &mpsc::UnboundedSender<ActivityEvent>, event: TraceEvent) {
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
    tx: &mpsc::UnboundedSender<ActivityEvent>,
    speed: f64,
) -> Result<()> {
    let start_time = Instant::now();
    let mut event_count = 0;

    while let Some(event) = stream.next_event()? {
        event_count += 1;

        let target_elapsed = if let Some(first_timestamp) = stream.first_timestamp {
            let time_offset = event.timestamp.signed_duration_since(first_timestamp);
            let target_elapsed_ms = time_offset.num_milliseconds().max(0) as f64 / speed;
            Duration::from_millis(target_elapsed_ms as u64)
        } else {
            Duration::from_millis(0)
        };

        let current_elapsed = start_time.elapsed();
        if target_elapsed > current_elapsed {
            let sleep_duration = target_elapsed - current_elapsed;
            sleep(sleep_duration).await;
        }

        process_event(tx, event);
    }

    info!("Replay finished. Processed {} total events.", event_count);

    Ok(())
}

async fn ctrl_c() {
    signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
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

    // Channel to signal TUI when replay is done
    let (backend_done_tx, backend_done_rx) = tokio::sync::oneshot::channel();

    info!("Spawning TUI");

    let mut tui_task = tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            match devenv_tui::TuiApp::new(rx, shutdown)
                .run(backend_done_rx)
                .await
            {
                Ok(_) => info!("TUI exited normally"),
                Err(e) => warn!("TUI error: {e}"),
            }
        }
    });

    info!("Starting trace replay from: {}", args.trace_file.display());

    let stream = TraceStream::new(file);

    tokio::select! {
        result = replay_events(stream, &tx, args.speed) => {
            if let Err(e) = result {
                warn!("Replay error: {e}");
            }
            // Signal TUI that replay is done
            let _ = backend_done_tx.send(());
        }
        _ = &mut tui_task => {
            info!("TUI exited");
        }
        _ = ctrl_c() => {
            info!("Interrupted");
            shutdown.shutdown();
            // Signal TUI that we're done (after interrupt)
            let _ = backend_done_tx.send(());
        }
    }

    // Restore terminal to normal state (disable raw mode, show cursor)
    devenv_tui::app::restore_terminal();

    Ok(())
}
