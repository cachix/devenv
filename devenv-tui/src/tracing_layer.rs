use crate::model_events::DataEvent;
use crate::model::{
    Activity, ActivityVariant, BuildActivity, DownloadActivity, QueryActivity,
};
use crate::tracing_interface::{details_fields, nix_fields, operation_fields, operation_types, task_fields};
use crate::{LogLevel, LogSource, NixActivityState, OperationId, OperationResult};
use rand;
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span,
};
use tracing_subscriber::{Layer, layer::Context};

/// Tracing layer that integrates with the TUI system via event queue
pub struct DevenvTuiLayer {
    event_tx: mpsc::UnboundedSender<DataEvent>,
}

impl DevenvTuiLayer {
    pub fn new(event_tx: mpsc::UnboundedSender<DataEvent>) -> Self {
        Self { event_tx }
    }

    /// Send an event to the model event queue
    fn send_event(&self, event: DataEvent) {
        // Ignore send errors - they only occur if the receiver has been dropped,
        // which happens during shutdown. We don't want to panic in that case.
        let _ = self.event_tx.send(event);
    }

    /// Create a BUILD activity with derivation and machine info
    fn create_build_activity(
        &self,
        activity_id: u64,
        operation_id: OperationId,
        operation_name: String,
        short_name: String,
        parent: Option<OperationId>,
        visitor: &FieldVisitor,
    ) -> Activity {
        let derivation = visitor.get_field(details_fields::DERIVATION);
        let machine = visitor.get_field(details_fields::MACHINE);

        ActivityBuilder::new(activity_id, operation_id, short_name, parent)
            .with_name(derivation.unwrap_or(operation_name))
            .with_detail(machine.as_ref().map(|m| format!("machine: {}", m)))
            .with_variant(ActivityVariant::Build(BuildActivity {
                phase: None,
                log_stdout_lines: Vec::new(),
                log_stderr_lines: Vec::new(),
            }))
            .build()
    }

    /// Create a DOWNLOAD activity with store path and substituter
    fn create_download_activity(
        &self,
        activity_id: u64,
        operation_id: OperationId,
        operation_name: String,
        short_name: String,
        parent: Option<OperationId>,
        visitor: &FieldVisitor,
    ) -> Activity {
        let store_path = visitor.get_field(details_fields::STORE_PATH);
        let substituter = visitor.get_field(details_fields::SUBSTITUTER);

        ActivityBuilder::new(activity_id, operation_id, short_name, parent)
            .with_name(store_path.unwrap_or(operation_name))
            .with_variant(ActivityVariant::Download(DownloadActivity {
                size_current: Some(0),
                size_total: None,
                speed: Some(0),
                substituter,
            }))
            .build()
    }

    /// Create a QUERY activity with store path and substituter
    fn create_query_activity(
        &self,
        activity_id: u64,
        operation_id: OperationId,
        operation_name: String,
        short_name: String,
        parent: Option<OperationId>,
        visitor: &FieldVisitor,
    ) -> Activity {
        let store_path = visitor.get_field(details_fields::STORE_PATH);
        let substituter = visitor.get_field(details_fields::SUBSTITUTER);

        ActivityBuilder::new(activity_id, operation_id, short_name, parent)
            .with_name(store_path.unwrap_or(operation_name))
            .with_variant(ActivityVariant::Query(QueryActivity { substituter }))
            .build()
    }

    /// Create a FETCH_TREE activity
    fn create_fetch_tree_activity(
        &self,
        activity_id: u64,
        operation_id: OperationId,
        operation_name: String,
        short_name: String,
        parent: Option<OperationId>,
    ) -> Activity {
        ActivityBuilder::new(activity_id, operation_id, short_name, parent)
            .with_name(operation_name)
            .with_variant(ActivityVariant::FetchTree)
            .build()
    }

    /// Create an EVALUATE activity
    fn create_evaluate_activity(
        &self,
        activity_id: u64,
        operation_id: OperationId,
        operation_name: String,
        short_name: String,
        parent: Option<OperationId>,
    ) -> Activity {
        ActivityBuilder::new(activity_id, operation_id, short_name, parent)
            .with_name(operation_name)
            .with_variant(ActivityVariant::Evaluating)
            .build()
    }

    /// Create a DEVENV user operation activity
    fn create_devenv_activity(
        &self,
        activity_id: u64,
        operation_id: OperationId,
        operation_name: String,
        short_name: String,
        parent: Option<OperationId>,
    ) -> Activity {
        ActivityBuilder::new(activity_id, operation_id, short_name, parent)
            .with_name(operation_name)
            .with_variant(ActivityVariant::UserOperation)
            .build()
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

/// Trait for extracting and parsing fields from tracing events
trait FieldExtractor {
    /// Extract activity ID, defaulting to 0 if not present or invalid
    fn activity_id_or_zero(&self) -> u64;

    /// Extract activity ID, defaulting to random value if not present or invalid
    fn activity_id_or_random(&self) -> u64;

    /// Extract field and clone it
    fn get_field(&self, key: &str) -> Option<String>;
}

impl FieldExtractor for FieldVisitor {
    fn activity_id_or_zero(&self) -> u64 {
        self.fields
            .get(nix_fields::ACTIVITY_ID)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    }

    fn activity_id_or_random(&self) -> u64 {
        self.fields
            .get(nix_fields::ACTIVITY_ID)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or_else(rand::random)
    }

    fn get_field(&self, key: &str) -> Option<String> {
        self.fields.get(key).cloned()
    }
}

/// Builder for creating Activity instances with flexible customization
struct ActivityBuilder {
    activity_id: u64,
    operation_id: OperationId,
    name: String,
    short_name: String,
    parent_operation: Option<OperationId>,
    detail: Option<String>,
    variant: ActivityVariant,
}

impl ActivityBuilder {
    /// Create a new builder with required fields
    fn new(
        activity_id: u64,
        operation_id: OperationId,
        short_name: String,
        parent_operation: Option<OperationId>,
    ) -> Self {
        Self {
            activity_id,
            operation_id: operation_id.clone(),
            name: operation_id.0,
            short_name,
            parent_operation,
            detail: None,
            variant: ActivityVariant::Unknown,
        }
    }

    /// Set the activity name
    fn with_name(mut self, name: String) -> Self {
        self.name = name;
        self
    }

    /// Set the activity detail
    fn with_detail(mut self, detail: Option<String>) -> Self {
        self.detail = detail;
        self
    }

    /// Set the activity variant
    fn with_variant(mut self, variant: ActivityVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Build the final Activity
    fn build(self) -> Activity {
        Activity {
            id: self.activity_id,
            operation_id: self.operation_id,
            name: self.name,
            short_name: self.short_name,
            parent_operation: self.parent_operation,
            start_time: Instant::now(),
            state: NixActivityState::Active,
            detail: self.detail,
            variant: self.variant,
            progress: None,
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

                // Create task activity directly instead of calling handle_task_start
                let activity = Activity {
                    id: activity_id,
                    operation_id,
                    name: task_name,
                    short_name: "Task".to_string(),
                    parent_operation: None, // add_activity will handle this
                    start_time: Instant::now(),
                    state: NixActivityState::Active,
                    detail: None,
                    variant: ActivityVariant::Task(crate::model::TaskActivity {
                        status: crate::model::TaskDisplayStatus::Running,
                        duration: None,
                    }),
                    progress: None,
                };

                self.send_event(DataEvent::AddActivity(activity));
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
                operation_types::BUILD => {
                    let activity_id = visitor.activity_id_or_zero();
                    span.extensions_mut().insert(activity_id);

                    // Register operation in hierarchy
                    self.send_event(DataEvent::RegisterOperation {
                        operation_id: operation_id.clone(),
                        operation_name: operation_name.clone(),
                        parent: parent.clone(),
                        fields: visitor.fields.clone(),
                    });

                    // Create and add activity
                    let activity = self.create_build_activity(
                        activity_id,
                        operation_id.clone(),
                        operation_name,
                        short_name,
                        parent,
                        &visitor,
                    );
                    self.send_event(DataEvent::AddActivity(activity));
                }
                operation_types::DOWNLOAD => {
                    let activity_id = visitor.activity_id_or_zero();
                    span.extensions_mut().insert(activity_id);

                    // Register operation in hierarchy
                    self.send_event(DataEvent::RegisterOperation {
                        operation_id: operation_id.clone(),
                        operation_name: operation_name.clone(),
                        parent: parent.clone(),
                        fields: visitor.fields.clone(),
                    });

                    // Create and add activity
                    let activity = self.create_download_activity(
                        activity_id,
                        operation_id.clone(),
                        operation_name,
                        short_name,
                        parent,
                        &visitor,
                    );
                    self.send_event(DataEvent::AddActivity(activity));
                }
                operation_types::QUERY => {
                    let activity_id = visitor.activity_id_or_zero();
                    span.extensions_mut().insert(activity_id);

                    // Register operation in hierarchy
                    self.send_event(DataEvent::RegisterOperation {
                        operation_id: operation_id.clone(),
                        operation_name: operation_name.clone(),
                        parent: parent.clone(),
                        fields: visitor.fields.clone(),
                    });

                    // Create and add activity
                    let activity = self.create_query_activity(
                        activity_id,
                        operation_id.clone(),
                        operation_name,
                        short_name,
                        parent,
                        &visitor,
                    );
                    self.send_event(DataEvent::AddActivity(activity));
                }
                operation_types::FETCH_TREE => {
                    let activity_id = visitor.activity_id_or_zero();
                    span.extensions_mut().insert(activity_id);

                    // Register operation in hierarchy
                    self.send_event(DataEvent::RegisterOperation {
                        operation_id: operation_id.clone(),
                        operation_name: operation_name.clone(),
                        parent: parent.clone(),
                        fields: visitor.fields.clone(),
                    });

                    // Create and add activity
                    let activity = self.create_fetch_tree_activity(
                        activity_id,
                        operation_id.clone(),
                        operation_name,
                        short_name,
                        parent,
                    );
                    self.send_event(DataEvent::AddActivity(activity));
                }
                operation_types::EVALUATE => {
                    let activity_id = visitor.activity_id_or_random();
                    span.extensions_mut().insert(activity_id);

                    // Register operation in hierarchy
                    self.send_event(DataEvent::RegisterOperation {
                        operation_id: operation_id.clone(),
                        operation_name: operation_name.clone(),
                        parent: parent.clone(),
                        fields: visitor.fields.clone(),
                    });

                    // Create and add activity
                    let activity = self.create_evaluate_activity(
                        activity_id,
                        operation_id.clone(),
                        operation_name,
                        short_name,
                        parent,
                    );
                    self.send_event(DataEvent::AddActivity(activity));
                }
                operation_types::DEVENV => {
                    let activity_id = rand::random();
                    span.extensions_mut().insert(activity_id);

                    // Register operation in hierarchy
                    self.send_event(DataEvent::RegisterOperation {
                        operation_id: operation_id.clone(),
                        operation_name: operation_name.clone(),
                        parent: parent.clone(),
                        fields: visitor.fields.clone(),
                    });

                    // Create and add activity (DEVENV uses direct activities.insert)
                    let activity = self.create_devenv_activity(
                        activity_id,
                        operation_id.clone(),
                        operation_name,
                        short_name,
                        parent,
                    );
                    self.send_event(DataEvent::AddActivity(activity));
                }
                _ => {
                    // Register operation without activity
                    self.send_event(DataEvent::RegisterOperation {
                        operation_id,
                        operation_name,
                        parent,
                        fields: visitor.fields.clone(),
                    });
                }
            }
        }
    }

    fn on_close(&self, id: span::Id, ctx: Context<'_, S>) {
        let span = ctx.span(&id).expect("Span not found");
        let metadata = span.metadata();

        // Capture close timestamp from event
        let end_time = Instant::now();

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

                    self.send_event(DataEvent::CompleteActivity {
                        activity_id,
                        success,
                        end_time,
                    });

                    // Clean up build logs for this activity
                    self.send_event(DataEvent::RemoveBuildLogs { activity_id });
                }
                "devenv.nix.download" if metadata.name() == "nix_download_start" => {
                    let activity_id = span.extensions().get::<u64>().copied().unwrap_or(0);

                    self.send_event(DataEvent::CompleteActivity {
                        activity_id,
                        success,
                        end_time,
                    });
                }
                "devenv.nix.query" if metadata.name() == "nix_query_start" => {
                    let activity_id = span.extensions().get::<u64>().copied().unwrap_or(0);

                    self.send_event(DataEvent::CompleteActivity {
                        activity_id,
                        success,
                        end_time,
                    });
                }
                "devenv.nix.fetch" if metadata.name() == "fetch_tree_start" => {
                    let activity_id = span.extensions().get::<u64>().copied().unwrap_or(0);

                    self.send_event(DataEvent::CompleteActivity {
                        activity_id,
                        success,
                        end_time,
                    });
                }
                _ => {
                    // Handle DEVENV operations with activities (created for user-facing spans)
                    if has_devenv_activity {
                        let activity_id = span.extensions().get::<u64>().copied().unwrap_or(0);

                        self.send_event(DataEvent::CompleteActivity {
                            activity_id,
                            success,
                            end_time,
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

                        self.send_event(DataEvent::CloseOperation {
                            operation_id: operation_id.clone(),
                            result,
                        });
                    }
                }
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        let target = event.metadata().target();
        let event_name = event.metadata().name();

        // Try to parse as a typed update
        if let Some(update) = crate::TracingUpdate::from_event(target, event_name, &visitor.fields) {
            self.send_event(DataEvent::ApplyTracingUpdate(update));
            return;
        }

        // Handle log messages marked with devenv.log = true
        if visitor.is_devenv_log {
            let message = visitor
                .fields
                .get("message")
                .unwrap_or(&"".to_string())
                .clone();

            use crate::LogMessage;
            let log_msg = LogMessage::new(
                LogLevel::from(*event.metadata().level()),
                message,
                LogSource::Tracing,
                visitor.fields,
            );
            self.send_event(DataEvent::AddLogMessage(log_msg));
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
