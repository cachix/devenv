use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use devenv_tui::TuiHandle;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tokio_shutdown::Shutdown;
use tracing::{info, warn};

/// JSON trace event from tracing-subscriber's JSON formatter
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

/// Span ID information
#[derive(Debug, Deserialize, Clone)]
struct SpanIds {
    span_id: String,
    parent_span_id: Option<String>,
}

/// Span attributes
#[derive(Debug, Deserialize, Clone)]
struct SpanAttrs {
    fields: HashMap<String, String>,
}

/// Processed trace event ready for model update
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
    /// Convert from raw JSON event to processed event
    fn from_raw(raw: RawTraceEvent) -> Self {
        // Start with event fields
        let mut fields = extract_string_map(&raw.fields);

        // Merge in span attributes if present (only emitted on first event for this span)
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

/// Extract all string-convertible values from a JSON map
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

/// Tracks the relationship between span IDs and operations/activities
#[derive(Debug)]
struct SpanTracker {
    /// Maps span_id to operation_id
    span_to_operation: HashMap<String, devenv_tui::OperationId>,
    /// Maps span_id to activity_id
    span_to_activity: HashMap<String, u64>,
    /// Counter for generating unique activity IDs
    next_activity_id: u64,
}

impl SpanTracker {
    fn new() -> Self {
        Self {
            span_to_operation: HashMap::new(),
            span_to_activity: HashMap::new(),
            next_activity_id: 1,
        }
    }

    fn register_operation(&mut self, span_id: String, operation_id: devenv_tui::OperationId) {
        self.span_to_operation.insert(span_id, operation_id);
    }

    fn register_activity(&mut self, span_id: String, activity_id: u64) {
        self.span_to_activity.insert(span_id, activity_id);
    }

    fn get_operation(&self, span_id: &str) -> Option<&devenv_tui::OperationId> {
        self.span_to_operation.get(span_id)
    }

    fn get_activity(&self, span_id: &str) -> Option<u64> {
        self.span_to_activity.get(span_id).copied()
    }

    fn generate_activity_id(&mut self) -> u64 {
        let id = self.next_activity_id;
        self.next_activity_id += 1;
        id
    }
}

/// Stream that reads JSON values line-by-line on demand
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

    /// Get the next event from the stream, or None if EOF
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

/// Processes trace events and updates the model
struct EventProcessor {
    span_tracker: SpanTracker,
    /// Track which span IDs we've already seen and created
    seen_spans: HashMap<String, bool>,
}

impl EventProcessor {
    fn new() -> Self {
        Self {
            span_tracker: SpanTracker::new(),
            seen_spans: HashMap::new(),
        }
    }

    /// Process a single trace event and update the model
    fn process(&mut self, event: &TraceEvent, model: &mut devenv_tui::model::Model) {
        let Some(span_id) = &event.span_id else {
            return;
        };

        // Check if this is the first time we're seeing this span
        let is_new_span = !self.seen_spans.contains_key(span_id);

        if is_new_span {
            self.seen_spans.insert(span_id.clone(), true);
            self.handle_span_creation(event, model);
        } else {
            self.handle_event_update(event, model);
        }
    }

    /// Handle creation of new spans (operations/activities)
    fn handle_span_creation(
        &mut self,
        event: &TraceEvent,
        model: &mut devenv_tui::model::Model,
    ) {
        let Some(span_id) = &event.span_id else {
            return;
        };

        // Check if this has explicit operation type/name fields
        let op_type = event.fields.get("devenv.ui.operation.type");
        let op_name = event.fields.get("devenv.ui.operation.name");

        // Only create operations/activities if they have explicit metadata
        if op_type.is_none() && op_name.is_none() {
            return;
        }

        // Determine parent operation from parent span
        let parent_operation = event
            .parent_span_id
            .as_ref()
            .and_then(|pid| self.span_tracker.get_operation(pid).cloned());

        let name = op_name.cloned().unwrap_or_else(|| "Unknown".to_string());

        match op_type.map(|s| s.as_str()) {
            Some("evaluate") => {
                self.create_evaluate_activity(span_id, &name, parent_operation, event, model);
            }
            Some("build") => {
                self.create_build_activity(span_id, &name, parent_operation, event, model);
            }
            Some("download") => {
                self.create_download_activity(span_id, &name, parent_operation, event, model);
            }
            Some("query") => {
                self.create_query_activity(span_id, &name, parent_operation, event, model);
            }
            Some("fetch_tree") => {
                self.create_fetch_tree_activity(span_id, &name, parent_operation, event, model);
            }
            _ => {
                // Generic operation with explicit name but no specific type
                self.create_generic_operation(span_id, &name, parent_operation, event, model);
            }
        }
    }

    /// Create an evaluate activity
    fn create_evaluate_activity(
        &mut self,
        span_id: &str,
        name: &str,
        parent_operation: Option<devenv_tui::OperationId>,
        _event: &TraceEvent,
        model: &mut devenv_tui::model::Model,
    ) {
        use devenv_tui::model::{Activity, ActivityVariant};

        let operation_id = devenv_tui::OperationId::new(span_id.to_string());
        self.span_tracker
            .register_operation(span_id.to_string(), operation_id.clone());

        // Create operation
        let operation = devenv_tui::Operation::new(
            operation_id.clone(),
            name.to_string(),
            parent_operation.clone(),
            HashMap::new(),
        );

        // Add to parent's children if applicable
        if let Some(parent_id) = &parent_operation {
            if let Some(parent_op) = model.operations.get_mut(parent_id) {
                parent_op.children.push(operation_id.clone());
            }
        } else {
            model.root_operations.push(operation_id.clone());
        }

        model.operations.insert(operation_id.clone(), operation);

        // Create evaluate activity
        let activity_id = self.span_tracker.generate_activity_id();
        self.span_tracker
            .register_activity(span_id.to_string(), activity_id);

        let activity = Activity {
            id: activity_id,
            operation_id: operation_id.clone(),
            name: name.to_string(),
            short_name: "Evaluating".to_string(),
            parent_operation,
            start_time: Instant::now(),
            state: devenv_tui::NixActivityState::Active,
            detail: Some("0 files".to_string()),
            variant: ActivityVariant::Evaluating,
            progress: None,
        };

        model.add_activity(activity);
    }

    /// Create a build activity
    fn create_build_activity(
        &mut self,
        span_id: &str,
        name: &str,
        parent_operation: Option<devenv_tui::OperationId>,
        event: &TraceEvent,
        model: &mut devenv_tui::model::Model,
    ) {
        use devenv_tui::model::{Activity, ActivityVariant, BuildActivity};

        let operation_id = devenv_tui::OperationId::new(span_id.to_string());
        self.span_tracker
            .register_operation(span_id.to_string(), operation_id.clone());

        let mut data = HashMap::new();
        if let Some(derivation) = event.fields.get("devenv.ui.details.derivation") {
            data.insert("derivation".to_string(), derivation.clone());
        }

        let operation = devenv_tui::Operation::new(
            operation_id.clone(),
            name.to_string(),
            parent_operation.clone(),
            data,
        );

        if let Some(parent_id) = &parent_operation {
            if let Some(parent_op) = model.operations.get_mut(parent_id) {
                parent_op.children.push(operation_id.clone());
            }
        } else {
            model.root_operations.push(operation_id.clone());
        }

        model.operations.insert(operation_id.clone(), operation);

        let activity_id = self.span_tracker.generate_activity_id();
        self.span_tracker
            .register_activity(span_id.to_string(), activity_id);

        let activity = Activity {
            id: activity_id,
            operation_id: operation_id.clone(),
            name: name.to_string(),
            short_name: "Building".to_string(),
            parent_operation,
            start_time: Instant::now(),
            state: devenv_tui::NixActivityState::Active,
            detail: event.fields.get("devenv.ui.details.derivation").cloned(),
            variant: ActivityVariant::Build(BuildActivity {
                phase: Some("preparing".to_string()),
                log_stdout_lines: Vec::new(),
                log_stderr_lines: Vec::new(),
            }),
            progress: None,
        };

        model.add_activity(activity);
    }

    /// Create a download activity
    fn create_download_activity(
        &mut self,
        span_id: &str,
        name: &str,
        parent_operation: Option<devenv_tui::OperationId>,
        event: &TraceEvent,
        model: &mut devenv_tui::model::Model,
    ) {
        use devenv_tui::model::{Activity, ActivityVariant, DownloadActivity};

        let operation_id = devenv_tui::OperationId::new(span_id.to_string());
        self.span_tracker
            .register_operation(span_id.to_string(), operation_id.clone());

        let mut data = HashMap::new();
        if let Some(store_path) = event.fields.get("devenv.ui.details.store_path") {
            data.insert("store_path".to_string(), store_path.clone());
        }
        if let Some(substituter) = event.fields.get("devenv.ui.details.substituter") {
            data.insert("substituter".to_string(), substituter.clone());
        }

        let operation = devenv_tui::Operation::new(
            operation_id.clone(),
            name.to_string(),
            parent_operation.clone(),
            data,
        );

        if let Some(parent_id) = &parent_operation {
            if let Some(parent_op) = model.operations.get_mut(parent_id) {
                parent_op.children.push(operation_id.clone());
            }
        } else {
            model.root_operations.push(operation_id.clone());
        }

        model.operations.insert(operation_id.clone(), operation);

        let activity_id = self.span_tracker.generate_activity_id();
        self.span_tracker
            .register_activity(span_id.to_string(), activity_id);

        let activity = Activity {
            id: activity_id,
            operation_id: operation_id.clone(),
            name: name.to_string(),
            short_name: "Downloading".to_string(),
            parent_operation,
            start_time: Instant::now(),
            state: devenv_tui::NixActivityState::Active,
            detail: event.fields.get("devenv.ui.details.store_path").cloned(),
            variant: ActivityVariant::Download(DownloadActivity {
                size_current: None,
                size_total: None,
                speed: None,
                substituter: event.fields.get("devenv.ui.details.substituter").cloned(),
            }),
            progress: None,
        };

        model.add_activity(activity);
    }

    /// Create a query activity
    fn create_query_activity(
        &mut self,
        span_id: &str,
        name: &str,
        parent_operation: Option<devenv_tui::OperationId>,
        event: &TraceEvent,
        model: &mut devenv_tui::model::Model,
    ) {
        use devenv_tui::model::{Activity, ActivityVariant, QueryActivity};

        let operation_id = devenv_tui::OperationId::new(span_id.to_string());
        self.span_tracker
            .register_operation(span_id.to_string(), operation_id.clone());

        let mut data = HashMap::new();
        if let Some(store_path) = event.fields.get("devenv.ui.details.store_path") {
            data.insert("store_path".to_string(), store_path.clone());
        }
        if let Some(substituter) = event.fields.get("devenv.ui.details.substituter") {
            data.insert("substituter".to_string(), substituter.clone());
        }

        let operation = devenv_tui::Operation::new(
            operation_id.clone(),
            name.to_string(),
            parent_operation.clone(),
            data,
        );

        if let Some(parent_id) = &parent_operation {
            if let Some(parent_op) = model.operations.get_mut(parent_id) {
                parent_op.children.push(operation_id.clone());
            }
        } else {
            model.root_operations.push(operation_id.clone());
        }

        model.operations.insert(operation_id.clone(), operation);

        let activity_id = self.span_tracker.generate_activity_id();
        self.span_tracker
            .register_activity(span_id.to_string(), activity_id);

        let activity = Activity {
            id: activity_id,
            operation_id: operation_id.clone(),
            name: name.to_string(),
            short_name: "Querying".to_string(),
            parent_operation,
            start_time: Instant::now(),
            state: devenv_tui::NixActivityState::Active,
            detail: event.fields.get("devenv.ui.details.store_path").cloned(),
            variant: ActivityVariant::Query(QueryActivity {
                substituter: event.fields.get("devenv.ui.details.substituter").cloned(),
            }),
            progress: None,
        };

        model.add_activity(activity);
    }

    /// Create a fetch_tree activity
    fn create_fetch_tree_activity(
        &mut self,
        span_id: &str,
        name: &str,
        parent_operation: Option<devenv_tui::OperationId>,
        _event: &TraceEvent,
        model: &mut devenv_tui::model::Model,
    ) {
        use devenv_tui::model::{Activity, ActivityVariant};

        let operation_id = devenv_tui::OperationId::new(span_id.to_string());
        self.span_tracker
            .register_operation(span_id.to_string(), operation_id.clone());

        let operation = devenv_tui::Operation::new(
            operation_id.clone(),
            name.to_string(),
            parent_operation.clone(),
            HashMap::new(),
        );

        if let Some(parent_id) = &parent_operation {
            if let Some(parent_op) = model.operations.get_mut(parent_id) {
                parent_op.children.push(operation_id.clone());
            }
        } else {
            model.root_operations.push(operation_id.clone());
        }

        model.operations.insert(operation_id.clone(), operation);

        let activity_id = self.span_tracker.generate_activity_id();
        self.span_tracker
            .register_activity(span_id.to_string(), activity_id);

        let activity = Activity {
            id: activity_id,
            operation_id: operation_id.clone(),
            name: name.to_string(),
            short_name: "Fetching".to_string(),
            parent_operation,
            start_time: Instant::now(),
            state: devenv_tui::NixActivityState::Active,
            detail: None,
            variant: ActivityVariant::FetchTree,
            progress: None,
        };

        model.add_activity(activity);
    }

    /// Create a generic operation (without activity)
    fn create_generic_operation(
        &mut self,
        span_id: &str,
        name: &str,
        parent_operation: Option<devenv_tui::OperationId>,
        _event: &TraceEvent,
        model: &mut devenv_tui::model::Model,
    ) {
        let operation_id = devenv_tui::OperationId::new(span_id.to_string());
        self.span_tracker
            .register_operation(span_id.to_string(), operation_id.clone());

        let operation = devenv_tui::Operation::new(
            operation_id.clone(),
            name.to_string(),
            parent_operation.clone(),
            HashMap::new(),
        );

        if let Some(parent_id) = &parent_operation {
            if let Some(parent_op) = model.operations.get_mut(parent_id) {
                parent_op.children.push(operation_id.clone());
            }
        } else {
            model.root_operations.push(operation_id.clone());
        }

        model.operations.insert(operation_id, operation);
    }

    /// Handle event updates (status, progress, etc.)
    fn handle_event_update(&mut self, event: &TraceEvent, model: &mut devenv_tui::model::Model) {
        let Some(span_id) = &event.span_id else {
            return;
        };

        // Check if this is a progress update
        if let Some(progress_type) = event.fields.get("devenv.ui.progress.type") {
            self.handle_progress_update(span_id, progress_type, event, model);
        }

        // Check if this is a status update
        if let Some(status) = event.fields.get("devenv.ui.status") {
            self.handle_status_update(span_id, status, event, model);
        }
    }

    /// Handle progress updates
    fn handle_progress_update(
        &mut self,
        span_id: &str,
        progress_type: &str,
        event: &TraceEvent,
        model: &mut devenv_tui::model::Model,
    ) {
        use devenv_tui::model::ProgressActivity;

        let Some(activity_id) = self.span_tracker.get_activity(span_id) else {
            return;
        };

        let Some(activity) = model.activities.get_mut(&activity_id) else {
            return;
        };

        match progress_type {
            "files" => {
                if let Some(current_str) = event.fields.get("devenv.ui.progress.current") {
                    if let Ok(current) = current_str.parse::<u64>() {
                        activity.progress = Some(ProgressActivity {
                            current: Some(current),
                            total: None,
                            unit: Some("files".to_string()),
                            percent: None,
                        });
                        activity.detail = Some(format!("{} files", current));
                    }
                }
            }
            "bytes" => {
                if let (Some(current_str), Some(total_str)) = (
                    event.fields.get("devenv.ui.progress.current"),
                    event.fields.get("devenv.ui.progress.total"),
                ) {
                    if let (Ok(current), Ok(total)) = (current_str.parse::<u64>(), total_str.parse::<u64>()) {
                        let percent = if total > 0 {
                            Some((current as f32 / total as f32) * 100.0)
                        } else {
                            None
                        };

                        activity.progress = Some(ProgressActivity {
                            current: Some(current),
                            total: Some(total),
                            unit: Some("bytes".to_string()),
                            percent,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    /// Handle status updates
    fn handle_status_update(
        &mut self,
        span_id: &str,
        status: &str,
        _event: &TraceEvent,
        model: &mut devenv_tui::model::Model,
    ) {
        // Update activity if one exists
        if let Some(activity_id) = self.span_tracker.get_activity(span_id) {
            if let Some(activity) = model.activities.get_mut(&activity_id) {
                match status {
                    "active" => {
                        activity.state = devenv_tui::NixActivityState::Active;
                    }
                    "completed" | "success" => {
                        let duration = activity.start_time.elapsed();
                        activity.state = devenv_tui::NixActivityState::Completed {
                            success: true,
                            duration,
                        };

                        // Also complete the operation
                        if let Some(operation) = model.operations.get_mut(&activity.operation_id) {
                            operation.complete(true);
                        }
                    }
                    "failed" | "error" => {
                        let duration = activity.start_time.elapsed();
                        activity.state = devenv_tui::NixActivityState::Completed {
                            success: false,
                            duration,
                        };

                        // Also complete the operation
                        if let Some(operation) = model.operations.get_mut(&activity.operation_id) {
                            operation.complete(false);
                        }
                    }
                    _ => {}
                }
            }
        }

        // If there's an operation but no activity, still complete the operation
        if let Some(operation_id) = self.span_tracker.get_operation(span_id) {
            match status {
                "completed" | "success" => {
                    if let Some(operation) = model.operations.get_mut(operation_id) {
                        operation.complete(true);
                    }
                }
                "failed" | "error" => {
                    if let Some(operation) = model.operations.get_mut(operation_id) {
                        operation.complete(false);
                    }
                }
                _ => {}
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
        .init();

    let shutdown = Shutdown::new();
    run_replay(tui_handle, shutdown).await?;

    Ok(())
}

async fn run_replay(tui_handle: TuiHandle, shutdown: std::sync::Arc<Shutdown>) -> Result<()> {
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

    let mut stream = TraceStream::new(file);

    // Get the first event to establish the baseline timestamp
    let first_event = stream
        .next_event()?
        .with_context(|| "No valid trace entries found")?;

    let first_timestamp = first_event.timestamp;
    info!("Starting trace replay from: {}", trace_path.display());

    // Start TUI in background
    let tui_task = tokio::spawn({
        let shutdown = shutdown.clone();
        let tui_handle_clone = tui_handle.clone();
        async move {
            match devenv_tui::app::run_app(tui_handle_clone, shutdown).await {
                Ok(_) => info!("TUI exited normally"),
                Err(e) => warn!("TUI error: {e}"),
            }
        }
    });

    // Replay trace entries with timing
    let start_time = Instant::now();
    let mut processor = EventProcessor::new();
    let mut event_count = 0;

    // Process the first event immediately
    if let Ok(mut model) = tui_handle.model().lock() {
        processor.process(&first_event, &mut model);
    }

    // Stream through remaining events
    while let Some(event) = stream.next_event()? {
        event_count += 1;
        if event_count % 100 == 0 {
            info!("Processed {} events", event_count);
        }
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

        // Process event and update model
        if let Ok(mut model) = tui_handle.model().lock() {
            processor.process(&event, &mut model);
        }
    }

    info!("Replay finished. Processed {} total events.", event_count + 1);
    info!("Press Ctrl+C to exit the TUI");

    // Wait for TUI to finish or shutdown
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
