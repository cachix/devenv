use crate::{
    message::{key_event_to_message, Message},
    model::{Activity, AppState, Model},
    ActivityProgress, LogMessage, NixActivityState, NixActivityType, Operation, OperationResult,
    TuiEvent,
};
use std::time::Instant;

/// Update function following The Elm Architecture
/// Takes the current model and a message, updates the model, and optionally returns a new message
pub fn update(model: &mut Model, msg: Message) -> Option<Message> {
    match msg {
        Message::TuiEvent(event) => handle_tui_event(model, event),

        Message::KeyEvent(key) => Some(key_event_to_message(key)),

        Message::UpdateSpinner => {
            let now = Instant::now();
            if now.duration_since(model.ui.last_spinner_update).as_millis() >= 50 {
                model.ui.spinner_frame = (model.ui.spinner_frame + 1) % 10;
                model.ui.last_spinner_update = now;
            }
            None
        }

        Message::SelectOperation(_id) => {
            // TODO: Update selection logic for activities
            None
        }

        Message::ClearSelection => {
            model.ui.selected_activity = None;
            None
        }

        Message::ToggleDetails => {
            model.ui.view_options.show_details = !model.ui.view_options.show_details;
            None
        }

        Message::ToggleExpandedLogs => {
            model.ui.view_options.show_expanded_logs = !model.ui.view_options.show_expanded_logs;
            None
        }

        Message::SelectNextActivity => {
            model.select_next_build();
            None
        }

        Message::SelectPreviousActivity => {
            model.select_previous_build();
            None
        }

        Message::RequestShutdown => {
            model.app_state = AppState::Shutdown;
            None
        }

        Message::None => None,
    }
}

/// Handle TUI events and update the model accordingly
fn handle_tui_event(model: &mut Model, event: TuiEvent) -> Option<Message> {
    match event {
        TuiEvent::OperationStart {
            id,
            message,
            parent,
            data,
        } => {
            let operation = Operation::new(id.clone(), message, parent.clone(), data);

            // Add to parent's children if parent exists
            if let Some(parent_id) = &parent {
                if let Some(parent_op) = model.operations.get_mut(parent_id) {
                    parent_op.children.push(id.clone());
                }
            } else {
                // Root operation - check if already exists
                if !model.root_operations.contains(&id) {
                    model.root_operations.push(id.clone());
                }
            }

            model.operations.insert(id, operation);
            None
        }

        TuiEvent::OperationEnd { id, result } => {
            if let Some(operation) = model.operations.get_mut(&id) {
                let success = matches!(result, OperationResult::Success);
                operation.complete(success);
            }
            None
        }

        TuiEvent::LogMessage {
            level,
            message,
            source,
            data,
        } => {
            let log_msg = LogMessage::new(level, message, source, data);
            model.add_log_message(log_msg);
            None
        }

        TuiEvent::NixBuildStart { .. } => None,
        TuiEvent::NixBuildProgress { .. } => None,
        TuiEvent::NixBuildEnd { .. } => None,

        TuiEvent::NixDerivationStart {
            operation_id,
            activity_id,
            derivation_path,
            derivation_name,
            machine,
        } => {
            let mut data = std::collections::HashMap::new();
            if let Some(machine) = machine {
                data.insert("machine".to_string(), machine);
            }

            let activity = Activity {
                id: activity_id,
                activity_type: NixActivityType::Build,
                operation_id: operation_id.clone(),
                name: derivation_path,
                short_name: derivation_name,
                parent_operation: model
                    .operations
                    .get(&operation_id)
                    .and_then(|op| op.parent.clone()),
                start_time: Instant::now(),
                state: NixActivityState::Active,
                progress: None,
                data,
            };
            model.activities.insert(activity_id, activity);
            None
        }

        TuiEvent::NixPhaseProgress {
            operation_id: _,
            activity_id,
            phase,
        } => {
            if let Some(activity) = model.activities.get_mut(&activity_id) {
                activity.data.insert("phase".to_string(), phase);
            }
            None
        }

        TuiEvent::NixDerivationEnd {
            operation_id: _,
            activity_id,
            success,
        } => {
            if let Some(activity) = model.activities.get_mut(&activity_id) {
                let duration = activity.start_time.elapsed();
                activity.state = NixActivityState::Completed { success, duration };
            }

            // Clean up build logs for this activity
            model.build_logs.remove(&activity_id);
            None
        }

        TuiEvent::NixDownloadStart {
            operation_id,
            activity_id,
            store_path,
            package_name,
            substituter,
        } => {
            let mut data = std::collections::HashMap::new();
            data.insert("substituter".to_string(), substituter);
            data.insert("bytes_downloaded".to_string(), "0".to_string());
            data.insert("download_speed".to_string(), "0.0".to_string());
            data.insert(
                "last_update_time".to_string(),
                format!("{:?}", Instant::now()),
            );
            data.insert("last_bytes_downloaded".to_string(), "0".to_string());

            let activity = Activity {
                id: activity_id,
                activity_type: NixActivityType::Download,
                operation_id: operation_id.clone(),
                name: store_path,
                short_name: package_name,
                parent_operation: model
                    .operations
                    .get(&operation_id)
                    .and_then(|op| op.parent.clone()),
                start_time: Instant::now(),
                state: NixActivityState::Active,
                progress: None,
                data,
            };
            model.activities.insert(activity_id, activity);
            None
        }

        TuiEvent::NixDownloadProgress {
            operation_id: _,
            activity_id,
            bytes_downloaded,
            total_bytes,
        } => {
            if let Some(activity) = model.activities.get_mut(&activity_id) {
                let now = Instant::now();

                // Calculate download speed
                if let (Some(_last_time_str), Some(last_bytes_str)) = (
                    activity.data.get("last_update_time"),
                    activity.data.get("last_bytes_downloaded"),
                ) {
                    // Simple time delta calculation (approximation)
                    if let Ok(last_bytes) = last_bytes_str.parse::<u64>() {
                        let time_delta = 0.1; // Assume ~100ms between updates
                        let bytes_delta = bytes_downloaded.saturating_sub(last_bytes) as f64;
                        let speed = bytes_delta / time_delta;
                        activity
                            .data
                            .insert("download_speed".to_string(), format!("{:.0}", speed));
                    }
                }

                activity
                    .data
                    .insert("bytes_downloaded".to_string(), bytes_downloaded.to_string());
                if let Some(total) = total_bytes {
                    activity
                        .data
                        .insert("total_bytes".to_string(), total.to_string());
                }
                activity
                    .data
                    .insert("last_update_time".to_string(), format!("{:?}", now));
                activity.data.insert(
                    "last_bytes_downloaded".to_string(),
                    bytes_downloaded.to_string(),
                );
            }
            None
        }

        TuiEvent::NixDownloadEnd {
            operation_id: _,
            activity_id,
            success,
        } => {
            if let Some(activity) = model.activities.get_mut(&activity_id) {
                let duration = activity.start_time.elapsed();
                activity.state = NixActivityState::Completed { success, duration };
            }
            None
        }

        TuiEvent::NixQueryStart {
            operation_id,
            activity_id,
            store_path,
            package_name,
            substituter,
        } => {
            let mut data = std::collections::HashMap::new();
            data.insert("substituter".to_string(), substituter);

            let activity = Activity {
                id: activity_id,
                activity_type: NixActivityType::Query,
                operation_id: operation_id.clone(),
                name: store_path,
                short_name: package_name,
                parent_operation: model
                    .operations
                    .get(&operation_id)
                    .and_then(|op| op.parent.clone()),
                start_time: Instant::now(),
                state: NixActivityState::Active,
                progress: None,
                data,
            };
            model.activities.insert(activity_id, activity);
            None
        }

        TuiEvent::NixQueryEnd {
            operation_id: _,
            activity_id,
            success,
        } => {
            if let Some(activity) = model.activities.get_mut(&activity_id) {
                let duration = activity.start_time.elapsed();
                activity.state = NixActivityState::Completed { success, duration };
            }
            None
        }

        TuiEvent::FetchTreeStart {
            operation_id,
            activity_id,
            message,
        } => {
            let activity = Activity {
                id: activity_id,
                activity_type: NixActivityType::FetchTree,
                operation_id: operation_id.clone(),
                name: message.clone(),
                short_name: message,
                parent_operation: model
                    .operations
                    .get(&operation_id)
                    .and_then(|op| op.parent.clone()),
                start_time: Instant::now(),
                state: NixActivityState::Active,
                progress: None,
                data: std::collections::HashMap::new(),
            };
            model.activities.insert(activity_id, activity);
            None
        }

        TuiEvent::FetchTreeEnd {
            operation_id: _,
            activity_id,
            success,
        } => {
            if let Some(activity) = model.activities.get_mut(&activity_id) {
                let duration = activity.start_time.elapsed();
                activity.state = NixActivityState::Completed { success, duration };
            }
            None
        }

        TuiEvent::BuildLog { activity_id, line } => {
            model.add_build_log(activity_id, line);
            None
        }

        TuiEvent::NixEvaluationStart {
            operation_id,
            file_path,
            total_files_evaluated,
        } => {
            // Update operation message to show evaluation started
            if let Some(operation) = model.operations.get_mut(&operation_id) {
                operation.message = file_path.to_string();
                operation
                    .data
                    .insert("evaluation_file".to_string(), file_path);
                operation.data.insert(
                    "evaluation_count".to_string(),
                    total_files_evaluated.to_string(),
                );
            }
            None
        }

        TuiEvent::NixEvaluationProgress {
            operation_id,
            files,
            total_files_evaluated,
        } => {
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
            None
        }

        TuiEvent::NixActivityProgress {
            operation_id: _,
            activity_id,
            done,
            expected,
            running,
            failed,
        } => {
            if let Some(activity) = model.activities.get_mut(&activity_id) {
                activity.progress = Some(ActivityProgress {
                    done,
                    expected,
                    running,
                    failed,
                });
            }
            None
        }

        TuiEvent::Shutdown => {
            model.app_state = AppState::Shutdown;
            None
        }
    }
}
