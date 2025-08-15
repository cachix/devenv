use crate::{LogLevel, LogSource, OperationId, OperationResult, TuiEvent};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{field::Visit, span, Subscriber};
use tracing_core::{Event, Field};
use tracing_subscriber::{layer::Context, Layer};

/// Tracing layer that integrates with the TUI system
pub struct DevenvTuiLayer {
    event_sender: mpsc::UnboundedSender<TuiEvent>,
}

impl DevenvTuiLayer {
    pub fn new(event_sender: mpsc::UnboundedSender<TuiEvent>) -> Self {
        Self { event_sender }
    }

    fn send_event(&self, event: TuiEvent) {
        if self.event_sender.send(event).is_err() {
            // TUI receiver has been dropped, which is expected during shutdown
        }
    }
}

/// Visitor to extract field values from tracing spans/events
#[derive(Default)]
struct FieldVisitor {
    fields: HashMap<String, String>,
    is_tui_op: bool,
    is_tui_log: bool,
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let field_name = field.name();
        let field_value = format!("{:?}", value);

        match field_name {
            "tui.op" => {
                self.is_tui_op = field_value.trim_matches('"') == "true";
            }
            "tui.log" => {
                self.is_tui_log = field_value.trim_matches('"') == "true";
            }
            _ => {
                self.fields.insert(field_name.to_string(), field_value);
            }
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let field_name = field.name();

        match field_name {
            "tui.op" => {
                self.is_tui_op = value == "true";
            }
            "tui.log" => {
                self.is_tui_log = value == "true";
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
            "tui.op" => {
                self.is_tui_op = value;
            }
            "tui.log" => {
                self.is_tui_log = value;
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

        // Handle spans marked with tui.op = true OR spans with devenv.user_message (legacy)
        let has_user_message = visitor.fields.contains_key("devenv.user_message");
        if visitor.is_tui_op || has_user_message {
            let operation_id = OperationId::new(format!("{}:{}", metadata.name(), id.into_u64()));
            let message = visitor
                .fields
                .get("message")
                .or_else(|| visitor.fields.get("devenv.user_message"))
                .unwrap_or(&metadata.name().to_string())
                .clone();

            // Find parent operation if any
            let parent = span
                .parent()
                .and_then(|parent_span| parent_span.extensions().get::<OperationId>().cloned());

            // Store operation ID in span extensions for children to find
            span.extensions_mut().insert(operation_id.clone());

            let event = TuiEvent::OperationStart {
                id: operation_id,
                message,
                parent,
                data: visitor.fields,
            };

            self.send_event(event);
        }
    }

    fn on_close(&self, id: span::Id, ctx: Context<'_, S>) {
        let span = ctx.span(&id).expect("Span not found");

        // Get operation ID first
        let operation_id = span.extensions().get::<OperationId>().cloned();

        if let Some(operation_id) = operation_id {
            // Determine success/failure based on whether an error was recorded
            let success = span.extensions().get::<SpanError>().is_none();

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

            let event = TuiEvent::OperationEnd {
                id: operation_id,
                result,
            };

            self.send_event(event);
        }
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        // Handle log messages marked with tui.log = true
        if visitor.is_tui_log {
            let message = visitor
                .fields
                .get("message")
                .unwrap_or(&"".to_string())
                .clone();

            let tui_event = TuiEvent::LogMessage {
                level: LogLevel::from(*event.metadata().level()),
                message,
                source: LogSource::Tracing,
                data: visitor.fields,
            };

            self.send_event(tui_event);
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
