use crate::Operation;
use crate::model::{
    Activity, ActivityVariant, BuildActivity, DownloadActivity, Model, ProgressActivity,
    QueryActivity,
};
use crate::tracing_interface::{
    details_fields, log_fields, nix_fields, operation_fields, operation_types, status_events,
    task_fields,
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
        if metadata.target().starts_with("devenv_tasks") || metadata.name() == "devenv_task" {
            // Check if this is a task execution span using formal interface constant
            if let Some(task_name) = visitor.fields.get(task_fields::NAME) {
                let task_name = task_name.trim_matches('"').to_string();
                let operation_id = OperationId::new(format!("task:{}", task_name));
                let activity_id = id.into_u64();

                self.update_model(|model| {
                    model.handle_task_start(task_name, Instant::now(), operation_id, activity_id);
                });
            }
        }

        // Handle spans with operation.type (standardized interface)
        let operation_type = visitor.fields.get(operation_fields::TYPE);
        if let Some(op_type) = operation_type {
            let operation_id = OperationId::new(format!("{}:{}", metadata.name(), id.into_u64()));

            let operation_name = visitor
                .fields
                .get(operation_fields::NAME)
                .cloned()
                .unwrap_or_else(|| metadata.name().to_string());

            let short_name = visitor
                .fields
                .get(operation_fields::SHORT_NAME)
                .cloned()
                .unwrap_or_else(|| operation_name.clone());

            // Find parent operation by traversing up the span hierarchy
            // We need to check all ancestors, not just the immediate parent,
            // because there might be intermediate spans (like "Running command") without operation_ids
            let parent = {
                let mut current = span.parent();
                let mut found_parent = None;

                while let Some(parent_span) = current {
                    if let Some(op_id) = parent_span.extensions().get::<OperationId>().cloned() {
                        found_parent = Some(op_id);
                        break;
                    }
                    current = parent_span.parent();
                }

                found_parent
            };

            // Store operation ID in span extensions for children to find
            span.extensions_mut().insert(operation_id.clone());

            // Handle specific operation types based on standardized operation.type field
            match op_type.as_str() {
                op_type_str if op_type_str == operation_types::BUILD => {
                    let derivation = visitor.fields.get(details_fields::DERIVATION).cloned();
                    let machine = visitor.fields.get(details_fields::MACHINE).cloned();
                    let activity_id = visitor
                        .fields
                        .get(nix_fields::ACTIVITY_ID)
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);

                    // Store activity_id in span extensions for later retrieval
                    span.extensions_mut().insert(activity_id);

                    self.update_model(|model| {
                        // Create Operation for the build
                        let operation = Operation::new(
                            operation_id.clone(),
                            operation_name.clone(),
                            parent.clone(),
                            visitor.fields.clone(),
                        );

                        // Add to parent's children if parent exists
                        if let Some(parent_id) = &parent {
                            if let Some(parent_op) = model.operations.get_mut(parent_id) {
                                parent_op.children.push(operation_id.clone());
                            }
                        } else {
                            // Root operation
                            if !model.root_operations.contains(&operation_id) {
                                model.root_operations.push(operation_id.clone());
                            }
                        }

                        model.operations.insert(operation_id.clone(), operation);

                        // Create Activity
                        let activity = Activity {
                            id: activity_id,
                            operation_id: operation_id.clone(),
                            name: derivation.clone().unwrap_or_else(|| operation_name.clone()),
                            short_name: short_name.clone(),
                            parent_operation: parent.clone(),
                            start_time: Instant::now(),
                            state: NixActivityState::Active,
                            detail: machine.as_ref().map(|m| format!("machine: {}", m)),
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
                op_type_str if op_type_str == operation_types::DOWNLOAD => {
                    let store_path = visitor.fields.get(details_fields::STORE_PATH).cloned();
                    let substituter = visitor.fields.get(details_fields::SUBSTITUTER).cloned();
                    let activity_id = visitor
                        .fields
                        .get(nix_fields::ACTIVITY_ID)
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);

                    // Store activity_id in span extensions for later retrieval
                    span.extensions_mut().insert(activity_id);

                    self.update_model(|model| {
                        // Create Operation for the download
                        let operation = Operation::new(
                            operation_id.clone(),
                            operation_name.clone(),
                            parent.clone(),
                            visitor.fields.clone(),
                        );

                        // Add to parent's children if parent exists
                        if let Some(parent_id) = &parent {
                            if let Some(parent_op) = model.operations.get_mut(parent_id) {
                                parent_op.children.push(operation_id.clone());
                            }
                        } else {
                            // Root operation
                            if !model.root_operations.contains(&operation_id) {
                                model.root_operations.push(operation_id.clone());
                            }
                        }

                        model.operations.insert(operation_id.clone(), operation);

                        // Create Activity
                        let activity = Activity {
                            id: activity_id,
                            operation_id: operation_id.clone(),
                            name: store_path.clone().unwrap_or_else(|| operation_name.clone()),
                            short_name: short_name.clone(),
                            parent_operation: parent.clone(),
                            start_time: Instant::now(),
                            state: NixActivityState::Active,
                            detail: None,
                            variant: ActivityVariant::Download(DownloadActivity {
                                size_current: Some(0),
                                size_total: None,
                                speed: Some(0),
                                substituter,
                            }),
                            progress: None,
                        };
                        model.add_activity(activity);
                    });
                }
                op_type_str if op_type_str == operation_types::QUERY => {
                    let store_path = visitor.fields.get(details_fields::STORE_PATH).cloned();
                    let substituter = visitor.fields.get(details_fields::SUBSTITUTER).cloned();
                    let activity_id = visitor
                        .fields
                        .get(nix_fields::ACTIVITY_ID)
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);

                    // Store activity_id in span extensions for later retrieval
                    span.extensions_mut().insert(activity_id);

                    self.update_model(|model| {
                        // Create Operation for the query
                        let operation = Operation::new(
                            operation_id.clone(),
                            operation_name.clone(),
                            parent.clone(),
                            visitor.fields.clone(),
                        );

                        // Add to parent's children if parent exists
                        if let Some(parent_id) = &parent {
                            if let Some(parent_op) = model.operations.get_mut(parent_id) {
                                parent_op.children.push(operation_id.clone());
                            }
                        } else {
                            // Root operation
                            if !model.root_operations.contains(&operation_id) {
                                model.root_operations.push(operation_id.clone());
                            }
                        }

                        model.operations.insert(operation_id.clone(), operation);

                        // Create Activity
                        let activity = Activity {
                            id: activity_id,
                            operation_id: operation_id.clone(),
                            name: store_path.clone().unwrap_or_else(|| operation_name.clone()),
                            short_name: short_name.clone(),
                            parent_operation: parent.clone(),
                            start_time: Instant::now(),
                            state: NixActivityState::Active,
                            detail: None,
                            variant: ActivityVariant::Query(QueryActivity { substituter }),
                            progress: None,
                        };
                        model.add_activity(activity);
                    });
                }
                op_type_str if op_type_str == operation_types::FETCH_TREE => {
                    let activity_id = visitor
                        .fields
                        .get(nix_fields::ACTIVITY_ID)
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);

                    // Store activity_id in span extensions for later retrieval
                    span.extensions_mut().insert(activity_id);

                    self.update_model(|model| {
                        // Create Operation for the fetch
                        let operation = Operation::new(
                            operation_id.clone(),
                            operation_name.clone(),
                            parent.clone(),
                            visitor.fields.clone(),
                        );

                        // Add to parent's children if parent exists
                        if let Some(parent_id) = &parent {
                            if let Some(parent_op) = model.operations.get_mut(parent_id) {
                                parent_op.children.push(operation_id.clone());
                            }
                        } else {
                            // Root operation
                            if !model.root_operations.contains(&operation_id) {
                                model.root_operations.push(operation_id.clone());
                            }
                        }

                        model.operations.insert(operation_id.clone(), operation);

                        // Create Activity
                        let activity = Activity {
                            id: activity_id,
                            operation_id: operation_id.clone(),
                            name: operation_name.clone(),
                            short_name: short_name.clone(),
                            parent_operation: parent.clone(),
                            start_time: Instant::now(),
                            state: NixActivityState::Active,
                            detail: None,
                            variant: ActivityVariant::FetchTree,
                            progress: None,
                        };
                        model.add_activity(activity);
                    });
                }
                op_type_str if op_type_str == operation_types::EVALUATE => {
                    let activity_id = visitor
                        .fields
                        .get(nix_fields::ACTIVITY_ID)
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or_else(rand::random);

                    // Store activity_id in span extensions for later retrieval
                    span.extensions_mut().insert(activity_id);

                    self.update_model(|model| {
                        // Create Operation for the evaluation
                        let operation = Operation::new(
                            operation_id.clone(),
                            operation_name.clone(),
                            parent.clone(),
                            visitor.fields.clone(),
                        );

                        // Add to parent's children if parent exists
                        if let Some(parent_id) = &parent {
                            if let Some(parent_op) = model.operations.get_mut(parent_id) {
                                parent_op.children.push(operation_id.clone());
                            }
                        } else {
                            // Root operation
                            if !model.root_operations.contains(&operation_id) {
                                model.root_operations.push(operation_id.clone());
                            }
                        }

                        model.operations.insert(operation_id.clone(), operation);

                        // Create Activity
                        let activity = Activity {
                            id: activity_id,
                            operation_id: operation_id.clone(),
                            name: operation_name.clone(),
                            short_name: short_name.clone(),
                            parent_operation: parent.clone(),
                            start_time: Instant::now(),
                            state: NixActivityState::Active,
                            detail: None,
                            variant: ActivityVariant::Evaluating,
                            progress: None,
                        };
                        model.add_activity(activity);
                    });
                }
                op_type_str if op_type_str == operation_types::DEVENV => {
                    // User-facing devenv messages - create both Operation and Activity for proper display
                    let activity_id = rand::random();

                    // Store activity_id in span extensions for later retrieval
                    span.extensions_mut().insert(activity_id);

                    self.update_model(|model| {
                        // Create Operation for proper hierarchy
                        let operation = Operation::new(
                            operation_id.clone(),
                            operation_name.clone(),
                            parent.clone(),
                            visitor.fields.clone(),
                        );

                        // Add to parent's children if parent exists
                        if let Some(parent_id) = &parent {
                            if let Some(parent_op) = model.operations.get_mut(parent_id) {
                                parent_op.children.push(operation_id.clone());
                            }
                        } else {
                            // Root operation - check if already exists
                            if !model.root_operations.contains(&operation_id) {
                                model.root_operations.push(operation_id.clone());
                            }
                        }

                        model.operations.insert(operation_id.clone(), operation);

                        // Create Activity
                        let activity = Activity {
                            id: activity_id,
                            operation_id: operation_id.clone(),
                            name: operation_name.clone(),
                            short_name: short_name.clone(),
                            parent_operation: parent.clone(),
                            start_time: Instant::now(),
                            state: NixActivityState::Active,
                            detail: None,
                            variant: ActivityVariant::UserOperation,
                            progress: None,
                        };
                        model.activities.insert(activity_id, activity);
                    });
                }
                _ => {
                    // Default operation start for other operation types
                    self.update_model(|model| {
                        let operation = Operation::new(
                            operation_id.clone(),
                            operation_name.clone(),
                            parent.clone(),
                            visitor.fields.clone(),
                        );

                        // Add to parent's children if parent exists
                        if let Some(parent_id) = &parent {
                            if let Some(parent_op) = model.operations.get_mut(parent_id) {
                                parent_op.children.push(operation_id.clone());
                            }
                        } else {
                            // Root operation - check if already exists
                            if !model.root_operations.contains(&operation_id) {
                                model.root_operations.push(operation_id.clone());
                            }
                        }

                        model.operations.insert(operation_id, operation);
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

            // Check if this is a DEVENV operation with activity tracking
            let has_devenv_activity = span.extensions().get::<u64>().is_some();

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
                    // Handle DEVENV operations with activities (created for user-facing spans)
                    if has_devenv_activity {
                        let activity_id = span.extensions().get::<u64>().copied().unwrap_or(0);

                        self.update_model(|model| {
                            // Update activity state
                            if let Some(activity) = model.activities.get_mut(&activity_id) {
                                let duration = activity.start_time.elapsed();
                                activity.state = NixActivityState::Completed { success, duration };
                            }
                        });
                    } else {
                        // Default operation end for other spans (without activities)
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
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        // Handle task events from devenv-tasks (using formal interface constants)
        if event.metadata().target().starts_with("devenv_tasks")
            && let Some(task_name) = visitor.fields.get(task_fields::NAME)
        {
            let task_name = task_name.trim_matches('"').to_string();

            // Check for task status updates
            if let Some(status) = visitor.fields.get(status_events::fields::STATUS) {
                let status = status.trim_matches('"').to_string();
                let result = visitor
                    .fields
                    .get(status_events::fields::RESULT)
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

                    let log_msg =
                        LogMessage::new(LogLevel::Info, message, LogSource::System, HashMap::new());
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
                    if let Some(activity_id_str) = visitor.fields.get("activity_id") {
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
                                if let Some(activity) = model.activities.get_mut(&activity_id) {
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
                    if let Some(activity_id_str) = visitor.fields.get("activity_id") {
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
                "devenv.nix.build" => {
                    // Handle phase updates
                    if let (Some(activity_id_str), Some(phase)) = (
                        visitor.fields.get("activity_id"),
                        visitor.fields.get("phase"),
                    ) {
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
                    // Handle build logs
                    if let (Some(activity_id_str), Some(line)) = (
                        visitor.fields.get("activity_id"),
                        visitor.fields.get("line"),
                    ) {
                        if let Ok(activity_id) = activity_id_str.parse::<u64>() {
                            self.update_model(|model| {
                                model.add_build_log(activity_id, line.clone());
                            });
                        }
                    }
                }
                "devenv.nix.eval" if event.metadata().name() == "nix_evaluation_progress" => {
                    if let (Some(activity_id_str), Some(files_str)) = (
                        visitor.fields.get(nix_fields::ACTIVITY_ID),
                        visitor.fields.get("files"),
                    ) {
                        if let Ok(activity_id) = activity_id_str.parse::<u64>() {
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
                                if let Some(activity) = model.activities.get_mut(&activity_id) {
                                    // Update progress with file count
                                    activity.progress = Some(ProgressActivity {
                                        current: Some(total_files_evaluated),
                                        total: None,
                                        unit: Some("files".to_string()),
                                        percent: None,
                                    });

                                    // Store latest file being evaluated in detail
                                    if let Some(latest_file) = files.last() {
                                        activity.detail = Some(latest_file.clone());
                                    }
                                }
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        // Handle log output events (stdout/stderr streaming)
        let target = event.metadata().target();
        if target == log_fields::STDOUT_TARGET || target == log_fields::STDERR_TARGET {
            if let (Some(stream), Some(message)) = (
                visitor.fields.get(log_fields::STREAM),
                visitor.fields.get(log_fields::MESSAGE),
            ) {
                // TODO: Associate with the correct task activity
                // For now, add to message log
                let log_level = if stream.contains("stderr") {
                    LogLevel::Warn
                } else {
                    LogLevel::Info
                };

                let log_msg = crate::LogMessage::new(
                    log_level,
                    message.clone(),
                    LogSource::Nix,
                    HashMap::new(),
                );

                self.update_model(|model| {
                    model.add_log_message(log_msg);
                });
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
