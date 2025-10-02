use crate::Operation;
use crate::model::{
    Activity, ActivityVariant, BuildActivity, DownloadActivity, Model, ProgressActivity,
    QueryActivity,
};
use crate::{LogLevel, LogSource, NixActivityState, OperationId, OperationResult};
use rand;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span,
};
use tracing_subscriber::{Layer, layer::Context};

/// Tracing layer that integrates with the TUI system
pub struct DevenvTuiLayer {
    model: Arc<Mutex<Model>>,
}

impl DevenvTuiLayer {
    pub fn new(model: Arc<Mutex<Model>>) -> Self {
        Self { model }
    }

    /// Helper to update the model with a closure (React-like state update)
    fn update_model<F>(&self, f: F)
    where
        F: FnOnce(&mut Model),
    {
        if let Ok(mut model) = self.model.lock() {
            f(&mut model);
        }
    }
}

/// Visitor to extract field values from tracing spans/events
#[derive(Default)]
struct FieldVisitor {
    fields: HashMap<String, String>,
    is_devenv_log: bool,
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let field_name = field.name();
        let field_value = format!("{:?}", value);

        match field_name {
            "devenv.log" => {
                self.is_devenv_log = field_value.trim_matches('"') == "true";
            }
            _ => {
                self.fields.insert(field_name.to_string(), field_value);
            }
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let field_name = field.name();

        match field_name {
            "devenv.log" => {
                self.is_devenv_log = value == "true";
            }
            _ => {
                self.fields
                    .insert(field_name.to_string(), value.to_string());
            }
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        let field_name = field.name();

        match field_name {
            "devenv.log" => {
                self.is_devenv_log = value;
            }
            _ => {
                self.fields
                    .insert(field_name.to_string(), value.to_string());
            }
        }
    }
}

impl<S> Layer<S> for DevenvTuiLayer
where
    S: Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        let metadata = attrs.metadata();
        let span = ctx.span(id).expect("Span not found");

        let mut visitor = FieldVisitor::default();
        attrs.record(&mut visitor);

        // Handle task-related spans from devenv-tasks
        if metadata.target().starts_with("devenv_tasks") {
            // Check if this is a task execution span
            if let Some(task_name) = visitor.fields.get("task_name") {
                let task_name = task_name.trim_matches('"').to_string();
                let operation_id = OperationId::new(format!("task:{}", task_name));
                let activity_id = id.into_u64();

                self.update_model(|model| {
                    model.handle_task_start(task_name, Instant::now(), operation_id, activity_id);
                });
            }
        }

        // Handle spans with devenv.ui.message
        let has_ui_message = visitor.fields.contains_key("devenv.ui.message");
        if has_ui_message {
            let operation_id = visitor
                .fields
                .get("devenv.ui.id")
                .cloned()
                .unwrap_or_else(|| format!("{}:{}", metadata.name(), id.into_u64()));
            let operation_id = OperationId::new(operation_id);

            let message = visitor
                .fields
                .get("devenv.ui.message")
                .unwrap_or(&metadata.name().to_string())
                .clone();

            // Find parent operation if any
            let parent = span
                .parent()
                .and_then(|parent_span| parent_span.extensions().get::<OperationId>().cloned());

            // Store operation ID in span extensions for children to find
            span.extensions_mut().insert(operation_id.clone());

            // Handle specific Nix event types
            let target = metadata.target();
            match target {
                "devenv.nix.build" if metadata.name() == "nix_derivation_start" => {
                    if let (Some(derivation_path), Some(derivation_name)) = (
                        visitor.fields.get("derivation_path"),
                        visitor.fields.get("derivation_name"),
                    ) {
                        let machine = visitor.fields.get("machine").cloned();
                        let activity_id = visitor
                            .fields
                            .get("activity_id")
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(0);

                        // Store activity_id in span extensions for later retrieval
                        span.extensions_mut().insert(activity_id);

                        let derivation_path_clone = derivation_path.clone();
                        let derivation_name_clone = derivation_name.clone();
                        let operation_id_clone = operation_id.clone();

                        self.update_model(|model| {
                            let activity = Activity {
                                id: activity_id,
                                operation_id: operation_id_clone,
                                name: derivation_path_clone,
                                short_name: derivation_name_clone,
                                parent_operation: None, // Let model handle this complex lookup
                                start_time: Instant::now(),
                                state: NixActivityState::Active,
                                detail: machine.map(|m| format!("machine: {}", m)),
                                variant: ActivityVariant::Build(BuildActivity {
                                    phase: None,
                                    log_stdout_lines: Vec::new(),
                                    log_stderr_lines: Vec::new(),
                                }),
                                progress: None,
                            };
                            model.add_activity(activity);
                        });
                    }
                }
                "devenv.nix.download" if metadata.name() == "nix_download_start" => {
                    if let (Some(store_path), Some(package_name), Some(substituter)) = (
                        visitor.fields.get("store_path"),
                        visitor.fields.get("package_name"),
                        visitor.fields.get("substituter"),
                    ) {
                        let activity_id = visitor
                            .fields
                            .get("activity_id")
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(0);

                        // Store activity_id in span extensions for later retrieval
                        span.extensions_mut().insert(activity_id);

                        let store_path_clone = store_path.clone();
                        let package_name_clone = package_name.clone();
                        let substituter_clone = substituter.clone();
                        let operation_id_clone = operation_id.clone();

                        self.update_model(|model| {
                            let activity = Activity {
                                id: activity_id,
                                operation_id: operation_id_clone.clone(),
                                name: store_path_clone,
                                short_name: package_name_clone,
                                parent_operation: model
                                    .operations
                                    .get(&operation_id_clone)
                                    .and_then(|op| op.parent.clone()),
                                start_time: Instant::now(),
                                state: NixActivityState::Active,
                                detail: None,
                                variant: ActivityVariant::Download(DownloadActivity {
                                    size_current: Some(0),
                                    size_total: None,
                                    speed: Some(0),
                                    substituter: Some(substituter_clone),
                                }),
                                progress: None,
                            };
                            model.activities.insert(activity_id, activity);
                        });
                    }
                }
                "devenv.nix.query" if metadata.name() == "nix_query_start" => {
                    if let (Some(store_path), Some(package_name), Some(substituter)) = (
                        visitor.fields.get("store_path"),
                        visitor.fields.get("package_name"),
                        visitor.fields.get("substituter"),
                    ) {
                        let activity_id = visitor
                            .fields
                            .get("activity_id")
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(0);

                        // Store activity_id in span extensions for later retrieval
                        span.extensions_mut().insert(activity_id);

                        // Query events are treated similar to download events but with query activity type
                        let store_path_clone = store_path.clone();
                        let package_name_clone = package_name.clone();
                        let substituter_clone = substituter.clone();
                        let operation_id_clone = operation_id.clone();

                        self.update_model(|model| {
                            let activity = Activity {
                                id: activity_id,
                                operation_id: operation_id_clone.clone(),
                                name: store_path_clone,
                                short_name: package_name_clone,
                                parent_operation: model
                                    .operations
                                    .get(&operation_id_clone)
                                    .and_then(|op| op.parent.clone()),
                                start_time: Instant::now(),
                                state: NixActivityState::Active,
                                detail: None,
                                variant: ActivityVariant::Query(QueryActivity {
                                    substituter: Some(substituter_clone),
                                }),
                                progress: None,
                            };
                            model.activities.insert(activity_id, activity);
                        });
                    }
                }
                "devenv.nix.fetch" if metadata.name() == "fetch_tree_start" => {
                    let activity_id = visitor
                        .fields
                        .get("activity_id")
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);

                    // Store activity_id in span extensions for later retrieval
                    span.extensions_mut().insert(activity_id);

                    let message_clone = message.clone();
                    let operation_id_clone = operation_id.clone();

                    self.update_model(|model| {
                        let activity = Activity {
                            id: activity_id,
                            operation_id: operation_id_clone.clone(),
                            name: message_clone.clone(),
                            short_name: message_clone,
                            parent_operation: model
                                .operations
                                .get(&operation_id_clone)
                                .and_then(|op| op.parent.clone()),
                            start_time: Instant::now(),
                            state: NixActivityState::Active,
                            detail: None,
                            variant: ActivityVariant::FetchTree,
                            progress: None,
                        };
                        model.activities.insert(activity_id, activity);
                    });
                }
                "devenv.nix.eval" if metadata.name() == "nix_evaluation_start" => {
                    if let Some(file_path) = visitor.fields.get("file_path") {
                        let activity_id = visitor
                            .fields
                            .get("activity_id")
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or_else(rand::random); // Generate if not provided

                        let total_files_evaluated = visitor
                            .fields
                            .get("total_files_evaluated")
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(0);

                        // Store activity_id in span extensions for later retrieval
                        span.extensions_mut().insert(activity_id);

                        let file_path_clone = file_path.clone();
                        let operation_id_clone = operation_id.clone();

                        // Create evaluation activity
                        self.update_model(|model| {
                            let activity = Activity {
                                id: activity_id,
                                operation_id: operation_id_clone.clone(),
                                name: format!("Evaluating {}", file_path_clone),
                                short_name: "Evaluation".to_string(),
                                parent_operation: model
                                    .operations
                                    .get(&operation_id_clone)
                                    .and_then(|op| op.parent.clone()),
                                start_time: Instant::now(),
                                state: NixActivityState::Active,
                                detail: if total_files_evaluated > 0 {
                                    Some(format!("{} files", total_files_evaluated))
                                } else {
                                    None
                                },
                                variant: ActivityVariant::Evaluating,
                                progress: None,
                            };
                            model.activities.insert(activity_id, activity);
                        });
                        self.update_model(|model| {
                            use crate::LogMessage;
                            let log_msg = LogMessage::new(
                                LogLevel::Info,
                                format!("Starting Nix evaluation: {}", file_path),
                                LogSource::Nix,
                                HashMap::new(),
                            );
                            model.add_log_message(log_msg);
                        });
                    }
                }
                _ => {
                    // Default operation start for other UI message spans
                    let operation_id_clone = operation_id.clone();
                    let message_clone = message.clone();
                    let parent_clone = parent.clone();
                    let data_clone = visitor.fields.clone();

                    self.update_model(|model| {
                        let operation = Operation::new(
                            operation_id_clone.clone(),
                            message_clone,
                            parent_clone.clone(),
                            data_clone,
                        );

                        // Add to parent's children if parent exists
                        if let Some(parent_id) = &parent_clone {
                            if let Some(parent_op) = model.operations.get_mut(parent_id) {
                                parent_op.children.push(operation_id_clone.clone());
                            }
                        } else {
                            // Root operation - check if already exists
                            if !model.root_operations.contains(&operation_id_clone) {
                                model.root_operations.push(operation_id_clone.clone());
                            }
                        }

                        model.operations.insert(operation_id_clone, operation);
                    });
                }
            }
        }
    }

    fn on_close(&self, id: span::Id, ctx: Context<'_, S>) {
        let span = ctx.span(&id).expect("Span not found");
        let metadata = span.metadata();

        // Get operation ID first
        let operation_id = span.extensions().get::<OperationId>().cloned();

        if let Some(operation_id) = operation_id {
            // Determine success/failure based on whether an error was recorded
            let success = span.extensions().get::<SpanError>().is_none();

            // Handle specific Nix end events
            let target = metadata.target();
            match target {
                "devenv.nix.build" if metadata.name() == "nix_derivation_start" => {
                    // Get activity_id from span extensions (stored during on_new_span)
                    let activity_id = span.extensions().get::<u64>().copied().unwrap_or(0);

                    self.update_model(|model| {
                        if let Some(activity) = model.activities.get_mut(&activity_id) {
                            let duration = activity.start_time.elapsed();
                            activity.state = NixActivityState::Completed { success, duration };
                        }

                        // Clean up build logs for this activity
                        model.build_logs.remove(&activity_id);
                    });
                }
                "devenv.nix.download" if metadata.name() == "nix_download_start" => {
                    let activity_id = span.extensions().get::<u64>().copied().unwrap_or(0);

                    self.update_model(|model| {
                        if let Some(activity) = model.activities.get_mut(&activity_id) {
                            let duration = activity.start_time.elapsed();
                            activity.state = NixActivityState::Completed { success, duration };
                        }
                    });
                }
                "devenv.nix.query" if metadata.name() == "nix_query_start" => {
                    let activity_id = span.extensions().get::<u64>().copied().unwrap_or(0);

                    self.update_model(|model| {
                        if let Some(activity) = model.activities.get_mut(&activity_id) {
                            let duration = activity.start_time.elapsed();
                            activity.state = NixActivityState::Completed { success, duration };
                        }
                    });
                }
                "devenv.nix.fetch" if metadata.name() == "fetch_tree_start" => {
                    let activity_id = span.extensions().get::<u64>().copied().unwrap_or(0);

                    self.update_model(|model| {
                        if let Some(activity) = model.activities.get_mut(&activity_id) {
                            let duration = activity.start_time.elapsed();
                            activity.state = NixActivityState::Completed { success, duration };
                        }
                    });
                }
                _ => {
                    // Default operation end for other spans
                    let result = if success {
                        OperationResult::Success
                    } else {
                        let error = span
                            .extensions()
                            .get::<SpanError>()
                            .cloned()
                            .unwrap_or_else(|| SpanError("Unknown error".to_string()));
                        OperationResult::Failure {
                            message: error.0,
                            code: None,
                            output: None,
                        }
                    };

                    let operation_id_clone = operation_id.clone();

                    self.update_model(|model| {
                        if let Some(operation) = model.operations.get_mut(&operation_id_clone) {
                            let success = matches!(result, OperationResult::Success);
                            operation.complete(success);
                        }
                    });
                }
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        // Handle task events from devenv-tasks
        if event.metadata().target().starts_with("devenv_tasks")
            && let Some(task_name) = visitor.fields.get("task_name") {
                let task_name = task_name.trim_matches('"').to_string();

                // Check for task status updates
                if let Some(status) = visitor.fields.get("status") {
                    let status = status.trim_matches('"').to_string();
                    let result = visitor
                        .fields
                        .get("result")
                        .map(|r| r.trim_matches('"').to_string());

                    // Update task status in the model
                    let task_name_clone = task_name.clone();
                    let status_clone = status.clone();
                    let result_clone = result.clone();

                    self.update_model(|model| {
                        use crate::LogMessage;
                        let message = if let Some(result) = result_clone {
                            format!(
                                "Task '{}' status: {} (result: {})",
                                task_name_clone, status_clone, result
                            )
                        } else {
                            format!("Task '{}' status: {}", task_name_clone, status_clone)
                        };

                        let log_msg = LogMessage::new(
                            LogLevel::Info,
                            message,
                            LogSource::System,
                            HashMap::new(),
                        );
                        model.add_log_message(log_msg);
                    });
                }

                // Check for task completion events (with duration and success)
                if let (Some(duration_secs), Some(success)) = (
                    visitor.fields.get("duration_secs"),
                    visitor.fields.get("success"),
                ) {
                    let duration_secs: f64 = duration_secs.trim_matches('"').parse().unwrap_or(0.0);
                    let duration = std::time::Duration::from_secs_f64(duration_secs);
                    let success = success.trim_matches('"') == "true";
                    let error = visitor
                        .fields
                        .get("error")
                        .map(|e| e.trim_matches('"').to_string());

                    self.update_model(|model| {
                        model.handle_task_end(task_name, Some(duration), success, error);
                    });
                }
            }

        // Handle Nix progress and other events
        let target = event.metadata().target();
        if target.starts_with("devenv.nix.") {
            match target {
                "devenv.nix.progress" => {
                    if let (Some(operation_id_str), Some(activity_id_str)) = (
                        visitor.fields.get("devenv.ui.id"),
                        visitor.fields.get("activity_id"),
                    ) {
                        let _operation_id = OperationId::new(operation_id_str.clone());
                        if let Ok(activity_id) = activity_id_str.parse::<u64>()
                            && let (Some(done_str), Some(expected_str)) =
                                (visitor.fields.get("done"), visitor.fields.get("expected"))
                                && let (Ok(done), Ok(expected)) =
                                    (done_str.parse::<u64>(), expected_str.parse::<u64>())
                                {
                                    let _running = visitor
                                        .fields
                                        .get("running")
                                        .and_then(|s| s.parse::<u64>().ok())
                                        .unwrap_or(0);
                                    let _failed = visitor
                                        .fields
                                        .get("failed")
                                        .and_then(|s| s.parse::<u64>().ok())
                                        .unwrap_or(0);

                                    self.update_model(|model| {
                                        if let Some(activity) =
                                            model.activities.get_mut(&activity_id)
                                        {
                                            activity.progress = Some(ProgressActivity {
                                                current: Some(done),
                                                total: Some(expected),
                                                unit: None,
                                                percent: if expected > 0 {
                                                    Some((done as f32 / expected as f32) * 100.0)
                                                } else {
                                                    None
                                                },
                                            });
                                        }
                                    });
                                }
                    }
                }
                "devenv.nix.download" if event.metadata().name() == "nix_download_progress" => {
                    if let (Some(operation_id_str), Some(activity_id_str)) = (
                        visitor.fields.get("devenv.ui.id"),
                        visitor.fields.get("activity_id"),
                    ) {
                        let _operation_id = OperationId::new(operation_id_str.clone());
                        if let Ok(activity_id) = activity_id_str.parse::<u64>() {
                            let bytes_downloaded = visitor
                                .fields
                                .get("bytes_downloaded")
                                .and_then(|s| s.parse::<u64>().ok())
                                .unwrap_or(0);
                            let total_bytes = visitor
                                .fields
                                .get("total_bytes")
                                .and_then(|s| s.parse::<u64>().ok());

                            self.update_model(|model| {
                                if let Some(activity) = model.activities.get_mut(&activity_id)
                                    && let ActivityVariant::Download(ref mut download_activity) =
                                        activity.variant
                                    {
                                        // Calculate download speed from previous values
                                        let speed =
                                            if let Some(current) = download_activity.size_current {
                                                let time_delta = 0.1; // Assume ~100ms between updates
                                                let bytes_delta =
                                                    bytes_downloaded.saturating_sub(current) as f64;
                                                (bytes_delta / time_delta) as u64
                                            } else {
                                                0
                                            };

                                        // Update download progress
                                        download_activity.size_current = Some(bytes_downloaded);
                                        download_activity.size_total = total_bytes;
                                        download_activity.speed = Some(speed);
                                    }
                            });
                        }
                    }
                }
                "devenv.nix.build" if event.metadata().name() == "nix_phase_progress" => {
                    if let (Some(operation_id_str), Some(activity_id_str), Some(phase)) = (
                        visitor.fields.get("devenv.ui.id"),
                        visitor.fields.get("activity_id"),
                        visitor.fields.get("phase"),
                    ) {
                        let _operation_id = OperationId::new(operation_id_str.clone());
                        if let Ok(activity_id) = activity_id_str.parse::<u64>() {
                            self.update_model(|model| {
                                if let Some(activity) = model.activities.get_mut(&activity_id)
                                    && let ActivityVariant::Build(ref mut build_activity) =
                                        activity.variant
                                    {
                                        build_activity.phase = Some(phase.clone());
                                    }
                            });
                        }
                    }
                }
                "devenv.nix.build" if event.metadata().name() == "build_log" => {
                    if let (Some(activity_id_str), Some(line)) = (
                        visitor.fields.get("activity_id"),
                        visitor.fields.get("line"),
                    )
                        && let Ok(activity_id) = activity_id_str.parse::<u64>() {
                            self.update_model(|model| {
                                model.add_build_log(activity_id, line.clone());
                            });
                        }
                }
                "devenv.nix.eval" if event.metadata().name() == "nix_evaluation_progress" => {
                    if let (Some(operation_id_str), Some(files_str)) = (
                        visitor.fields.get("devenv.ui.id"),
                        visitor.fields.get("files"),
                    ) {
                        let operation_id = OperationId::new(operation_id_str.clone());
                        let total_files_evaluated = visitor
                            .fields
                            .get("total_files_evaluated")
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(0);

                        // Parse the files array - this is a bit crude but should work
                        let files: Vec<String> = files_str
                            .trim_start_matches('[')
                            .trim_end_matches(']')
                            .split(", ")
                            .map(|s| s.trim_matches('"').to_string())
                            .filter(|s| !s.is_empty())
                            .collect();

                        self.update_model(|model| {
                            if let Some(operation) = model.operations.get_mut(&operation_id) {
                                // Since files are in evaluation order, the last one is the most recent
                                if let Some(latest_file) = files.last() {
                                    operation.message = latest_file.to_string();
                                    operation
                                        .data
                                        .insert("evaluation_file".to_string(), latest_file.clone());
                                }
                                operation.data.insert(
                                    "evaluation_count".to_string(),
                                    total_files_evaluated.to_string(),
                                );
                            }
                        });
                    }
                }
                _ => {}
            }
        }

        // Handle log messages marked with devenv.log = true
        if visitor.is_devenv_log {
            let message = visitor
                .fields
                .get("message")
                .unwrap_or(&"".to_string())
                .clone();

            self.update_model(|model| {
                use crate::LogMessage;
                let log_msg = LogMessage::new(
                    LogLevel::from(*event.metadata().level()),
                    message,
                    LogSource::Tracing,
                    visitor.fields,
                );
                model.add_log_message(log_msg);
            });
        }
    }

    fn on_record(&self, id: &span::Id, values: &span::Record<'_>, ctx: Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found");

        // Check for error recording
        let mut visitor = ErrorVisitor::default();
        values.record(&mut visitor);

        if let Some(error) = visitor.error {
            span.extensions_mut().insert(SpanError(error));
        }
    }
}

/// Visitor to detect errors in span records
#[derive(Default)]
struct ErrorVisitor {
    error: Option<String>,
}

impl Visit for ErrorVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "error" {
            self.error = Some(format!("{:?}", value));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "error" {
            self.error = Some(value.to_string());
        }
    }
}

/// Extension to mark spans that encountered errors
#[derive(Clone)]
struct SpanError(String);
