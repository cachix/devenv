use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use devenv_activity::Timestamp;
use devenv_activity::{ActivityEvent, ActivityKind, ActivityOutcome};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_shutdown::Shutdown;
use tracing::{info, warn};

#[derive(Debug, Deserialize, Clone)]
struct RawTraceEvent {
    timestamp: DateTime<Utc>,
    #[serde(default)]
    level: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    fields: HashMap<String, serde_json::Value>,
    #[serde(default)]
    span_ids: Option<SpanIds>,
    #[serde(default)]
    span_attrs: Option<SpanAttrs>,
}

#[derive(Debug, Deserialize, Clone)]
struct SpanIds {
    span_id: String,
    parent_span_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct SpanAttrs {
    fields: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct TraceEvent {
    timestamp: DateTime<Utc>,
    #[allow(dead_code)]
    level: String,
    #[allow(dead_code)]
    target: String,
    fields: HashMap<String, String>,
    span_id: Option<String>,
    parent_span_id: Option<String>,
}

impl TraceEvent {
    fn from_raw(raw: RawTraceEvent) -> Self {
        let mut fields = extract_string_map(&raw.fields);

        if let Some(span_attrs) = raw.span_attrs {
            for (key, value) in span_attrs.fields {
                fields.entry(key).or_insert(value);
            }
        }

        let (span_id, parent_span_id) = raw
            .span_ids
            .map(|ids| (Some(ids.span_id), ids.parent_span_id))
            .unwrap_or((None, None));

        Self {
            timestamp: raw.timestamp,
            level: raw.level,
            target: raw.target,
            fields,
            span_id,
            parent_span_id,
        }
    }
}

fn extract_string_map(json_map: &HashMap<String, serde_json::Value>) -> HashMap<String, String> {
    json_map
        .iter()
        .filter_map(|(k, v)| {
            v.as_str()
                .map(|s| (k.clone(), s.to_string()))
                .or_else(|| v.as_u64().map(|n| (k.clone(), n.to_string())))
                .or_else(|| v.as_i64().map(|n| (k.clone(), n.to_string())))
        })
        .collect()
}

#[derive(Debug)]
struct SpanTracker {
    span_to_activity: HashMap<String, u64>,
    next_activity_id: u64,
}

impl SpanTracker {
    fn new() -> Self {
        Self {
            span_to_activity: HashMap::new(),
            next_activity_id: 1,
        }
    }

    fn register_activity(&mut self, span_id: String) -> u64 {
        let id = self.next_activity_id;
        self.next_activity_id += 1;
        self.span_to_activity.insert(span_id, id);
        id
    }

    fn get_activity(&self, span_id: &str) -> Option<u64> {
        self.span_to_activity.get(span_id).copied()
    }

    fn get_parent_activity(&self, parent_span_id: &Option<String>) -> Option<u64> {
        parent_span_id
            .as_ref()
            .and_then(|pid| self.span_to_activity.get(pid).copied())
    }
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

                    match serde_json::from_str::<RawTraceEvent>(&line) {
                        Ok(raw_event) => {
                            let event = TraceEvent::from_raw(raw_event);

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
    span_tracker: SpanTracker,
    seen_spans: HashMap<String, bool>,
    tx: mpsc::UnboundedSender<ActivityEvent>,
}

impl EventProcessor {
    fn new(tx: mpsc::UnboundedSender<ActivityEvent>) -> Self {
        Self {
            span_tracker: SpanTracker::new(),
            seen_spans: HashMap::new(),
            tx,
        }
    }

    fn send(&self, event: ActivityEvent) {
        let _ = self.tx.send(event);
    }

    fn process(&mut self, event: &TraceEvent) {
        let Some(span_id) = &event.span_id else {
            return;
        };

        let is_new_span = !self.seen_spans.contains_key(span_id);

        if is_new_span {
            self.seen_spans.insert(span_id.clone(), true);
            self.handle_span_creation(event);
        } else {
            self.handle_event_update(event);
        }
    }

    fn handle_span_creation(&mut self, event: &TraceEvent) {
        let Some(span_id) = &event.span_id else {
            return;
        };

        let op_type = event.fields.get("devenv.ui.operation.type");
        let op_name = event.fields.get("devenv.ui.operation.name");

        if op_type.is_none() && op_name.is_none() {
            return;
        }

        let name = op_name.cloned().unwrap_or_else(|| "Unknown".to_string());
        let parent = self.span_tracker.get_parent_activity(&event.parent_span_id);
        let activity_id = self.span_tracker.register_activity(span_id.clone());

        let kind = match op_type.map(|s| s.as_str()) {
            Some("evaluate") => ActivityKind::Evaluate,
            Some("build") => ActivityKind::Build,
            Some("download") => ActivityKind::Fetch,
            Some("query") => ActivityKind::Operation,
            Some("fetch_tree") => ActivityKind::Fetch,
            Some("task") => ActivityKind::Task,
            _ => ActivityKind::Operation,
        };

        let detail = event
            .fields
            .get("devenv.ui.details.derivation")
            .or_else(|| event.fields.get("devenv.ui.details.store_path"))
            .cloned();

        self.send(ActivityEvent::Start {
            id: activity_id,
            kind,
            name,
            parent,
            detail,
            timestamp: Timestamp::now(),
        });
    }

    fn handle_event_update(&mut self, event: &TraceEvent) {
        let Some(span_id) = &event.span_id else {
            return;
        };

        let Some(activity_id) = self.span_tracker.get_activity(span_id) else {
            return;
        };

        if let Some(progress_type) = event.fields.get("devenv.ui.progress.type") {
            self.handle_progress_update(activity_id, progress_type, event);
        }

        if let Some(status) = event.fields.get("devenv.ui.status") {
            self.handle_status_update(activity_id, status);
        }

        if let Some(phase) = event.fields.get("devenv.ui.details.phase") {
            self.send(ActivityEvent::Phase {
                id: activity_id,
                phase: phase.clone(),
                timestamp: Timestamp::now(),
            });
        }
    }

    fn handle_progress_update(
        &mut self,
        activity_id: u64,
        progress_type: &str,
        event: &TraceEvent,
    ) {
        let current = event
            .fields
            .get("devenv.ui.progress.current")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let total = event
            .fields
            .get("devenv.ui.progress.total")
            .and_then(|s| s.parse::<u64>().ok());

        let progress = match (progress_type, total) {
            ("bytes", Some(total)) => devenv_activity::ProgressState::Determinate {
                current,
                total,
                unit: Some(devenv_activity::ProgressUnit::Bytes),
            },
            ("files", _) => devenv_activity::ProgressState::Indeterminate {
                current,
                unit: Some(devenv_activity::ProgressUnit::Files),
            },
            (_, Some(total)) => devenv_activity::ProgressState::Determinate {
                current,
                total,
                unit: None,
            },
            _ => devenv_activity::ProgressState::Indeterminate {
                current,
                unit: None,
            },
        };

        self.send(ActivityEvent::Progress {
            id: activity_id,
            progress,
            timestamp: Timestamp::now(),
        });
    }

    fn handle_status_update(&mut self, activity_id: u64, status: &str) {
        let outcome = match status {
            "completed" | "success" => Some(ActivityOutcome::Success),
            "failed" | "error" => Some(ActivityOutcome::Failed),
            "cancelled" => Some(ActivityOutcome::Cancelled),
            _ => None,
        };

        if let Some(outcome) = outcome {
            self.send(ActivityEvent::Complete {
                id: activity_id,
                outcome,
                timestamp: Timestamp::now(),
            });
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry().init();

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

    // Create activity channel - replay sends events, TUI receives them
    let (tx, rx) = mpsc::unbounded_channel();

    let shutdown = Shutdown::new();

    // Start TUI in background
    let tui_task = tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            match devenv_tui::app::run_app(rx, shutdown).await {
                Ok(_) => info!("TUI exited normally"),
                Err(e) => warn!("TUI error: {e}"),
            }
        }
    });

    // Process trace file and send events
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
