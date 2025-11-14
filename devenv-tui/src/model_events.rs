use crate::events::{LogMessage, Operation, OperationId, OperationResult};
use crate::model::Activity;
use iocraft::KeyCode;
use std::collections::HashMap;
use std::time::Instant;

/// High-volume data events from tracing layer
///
/// These events represent external data flowing into the system (builds, downloads, logs).
/// They can be batched for efficient processing without affecting UI responsiveness.
#[derive(Debug, Clone)]
pub enum DataEvent {
    /// Register a new operation in the hierarchy
    RegisterOperation {
        operation_id: OperationId,
        operation_name: String,
        parent: Option<OperationId>,
        fields: HashMap<String, String>,
    },

    /// Close an operation and mark it complete
    CloseOperation {
        operation_id: OperationId,
        result: OperationResult,
    },

    /// Add a new activity to the model
    AddActivity(Activity),

    /// Mark an activity as complete with timestamp
    CompleteActivity {
        activity_id: u64,
        success: bool,
        end_time: Instant,
    },

    /// Clean up build logs for an activity (when build completes)
    RemoveBuildLogs { activity_id: u64 },

    /// Apply a typed tracing update (progress, logs, etc.)
    ApplyTracingUpdate(crate::TracingUpdate),

    /// Add a general log message
    AddLogMessage(LogMessage),
}

/// Low-volume UI control events
///
/// These events represent user interactions and UI state changes.
/// They are processed immediately with priority over data events for responsiveness.
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// Keyboard input from user
    KeyInput(KeyCode),

    /// Animation tick for spinner updates
    Tick,
}

impl DataEvent {
    /// Process this data event by applying it to the model
    ///
    /// Data events represent external data flowing into the system.
    /// They can be batched for efficient processing.
    pub fn apply(self, model: &mut crate::Model) {
        match self {
            DataEvent::RegisterOperation {
                operation_id,
                operation_name,
                parent,
                fields,
            } => {
                let operation = Operation::new(
                    operation_id.clone(),
                    operation_name,
                    parent.clone(),
                    fields,
                );

                // Add to parent's children if parent exists
                if let Some(parent_id) = &parent {
                    if let Some(parent_op) = model.operations.get_mut(parent_id) {
                        if !parent_op.children.contains(&operation_id) {
                            parent_op.children.push(operation_id.clone());
                        }
                    }
                } else {
                    // No parent, so this is a root operation
                    if !model.root_operations.contains(&operation_id) {
                        model.root_operations.push(operation_id.clone());
                    }
                }

                // Insert the operation itself
                model.operations.insert(operation_id, operation);
            }

            DataEvent::CloseOperation {
                operation_id,
                result,
            } => {
                if let Some(operation) = model.operations.get_mut(&operation_id) {
                    let success = matches!(result, OperationResult::Success);
                    operation.complete(success);
                }
            }

            DataEvent::AddActivity(activity) => {
                model.add_activity(activity);
            }

            DataEvent::CompleteActivity {
                activity_id,
                success,
                end_time,
            } => {
                if let Some(activity) = model.activities.get_mut(&activity_id) {
                    // Calculate duration from timestamp pair
                    let duration = end_time.duration_since(activity.start_time);
                    activity.state = crate::NixActivityState::Completed { success, duration };
                }
            }

            DataEvent::RemoveBuildLogs { activity_id } => {
                model.build_logs.remove(&activity_id);
            }

            DataEvent::ApplyTracingUpdate(update) => {
                model.apply_update(update);
            }

            DataEvent::AddLogMessage(log_msg) => {
                model.add_log_message(log_msg);
            }
        }
    }
}

impl UiEvent {
    /// Process this UI event by applying it to the model
    ///
    /// UI events are processed immediately with priority for responsiveness.
    pub fn apply(self, model: &mut crate::Model) {
        match self {
            UiEvent::KeyInput(key_code) => {
                use KeyCode::*;
                match key_code {
                    Down => {
                        model.select_next_build();
                    }
                    Up => {
                        model.select_previous_build();
                    }
                    Esc => {
                        model.ui.selected_activity = None;
                    }
                    Char('e') => {
                        model.ui.view_options.show_expanded_logs =
                            !model.ui.view_options.show_expanded_logs;
                    }
                    _ => {}
                }
            }

            UiEvent::Tick => {
                // Update spinner animation
                let now = std::time::Instant::now();
                if now
                    .duration_since(model.ui.last_spinner_update)
                    .as_millis()
                    >= 50
                {
                    model.ui.spinner_frame = (model.ui.spinner_frame + 1) % 10;
                    model.ui.last_spinner_update = now;
                }
            }
        }
    }
}
