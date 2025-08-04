use crate::{
    message::{key_event_to_message, Message},
    model::{AppState, Model},
    ActivityProgress, FetchTreeInfo, LogMessage, NixActivityState, NixBuildInfo, NixDerivationInfo,
    NixDownloadInfo, NixQueryInfo, Operation, OperationResult, TuiEvent,
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

        Message::SelectOperation(id) => {
            model.ui.selected_operation = Some(id);
            None
        }

        Message::ClearSelection => {
            model.ui.selected_operation = None;
            model.ui.selected_activity_index = None;
            None
        }

        Message::ToggleDetails => {
            model.ui.show_details = !model.ui.show_details;
            None
        }

        Message::ToggleExpandedLogs => {
            model.ui.show_expanded_logs = !model.ui.show_expanded_logs;
            None
        }

        Message::SelectNextActivity => None,

        Message::SelectPreviousActivity => None,

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
                let duration = operation.start_time.elapsed();
                operation.complete(success);

                // Print completion message to stderr (above the TUI area)
                use std::io::Write;
                let symbol = if success { "✓" } else { "✖" };
                let color = if success { "\x1b[32m" } else { "\x1b[31m" };
                let reset = "\x1b[0m";
                let duration_str = crate::view::format_duration(duration);
                let _ = writeln!(
                    std::io::stderr(),
                    "{}{}{} {} in {}",
                    color,
                    symbol,
                    reset,
                    operation.message,
                    duration_str
                );
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

        TuiEvent::NixBuildStart {
            operation_id,
            derivation,
            machine: _,
        } => {
            let build_info = NixBuildInfo {
                operation_id: operation_id.clone(),
                derivation,
                current_phase: None,
                start_time: Instant::now(),
            };
            model.nix_builds.insert(operation_id, build_info);
            None
        }

        TuiEvent::NixBuildProgress {
            operation_id,
            phase,
        } => {
            if let Some(build_info) = model.nix_builds.get_mut(&operation_id) {
                build_info.current_phase = Some(phase);
            }
            None
        }

        TuiEvent::NixBuildEnd {
            operation_id,
            success: _,
        } => {
            model.nix_builds.remove(&operation_id);
            None
        }

        TuiEvent::NixDerivationStart {
            operation_id,
            activity_id,
            derivation_path,
            derivation_name,
            machine,
        } => {
            let derivation_info = NixDerivationInfo {
                operation_id,
                activity_id,
                derivation_path,
                derivation_name,
                machine,
                current_phase: None,
                start_time: Instant::now(),
                state: NixActivityState::Active,
            };
            model.nix_derivations.insert(activity_id, derivation_info);
            None
        }

        TuiEvent::NixPhaseProgress {
            operation_id: _,
            activity_id,
            phase,
        } => {
            if let Some(derivation_info) = model.nix_derivations.get_mut(&activity_id) {
                derivation_info.current_phase = Some(phase);
            }
            None
        }

        TuiEvent::NixDerivationEnd {
            operation_id: _,
            activity_id,
            success,
        } => {
            if let Some(derivation_info) = model.nix_derivations.get_mut(&activity_id) {
                let duration = derivation_info.start_time.elapsed();
                derivation_info.state = NixActivityState::Completed { success, duration };

                // Print completion message
                use std::io::Write;
                let symbol = if success { "✓" } else { "✖" };
                let color = if success { "\x1b[32m" } else { "\x1b[31m" };
                let reset = "\x1b[0m";
                let duration_str = crate::view::format_duration(duration);
                let _ = writeln!(
                    std::io::stderr(),
                    "{}{}{} Built {} in {}",
                    color,
                    symbol,
                    reset,
                    derivation_info.derivation_name,
                    duration_str
                );
            }

            // Clean up progress data
            model.activity_progress.remove(&activity_id);

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
            let now = Instant::now();
            let download_info = NixDownloadInfo {
                operation_id,
                activity_id,
                store_path,
                package_name,
                substituter,
                bytes_downloaded: 0,
                total_bytes: None,
                start_time: now,
                state: NixActivityState::Active,
                last_update_time: now,
                last_bytes_downloaded: 0,
                download_speed: 0.0,
            };
            model.nix_downloads.insert(activity_id, download_info);
            None
        }

        TuiEvent::NixDownloadProgress {
            operation_id: _,
            activity_id,
            bytes_downloaded,
            total_bytes,
        } => {
            if let Some(download_info) = model.nix_downloads.get_mut(&activity_id) {
                let now = Instant::now();
                let time_delta = now
                    .duration_since(download_info.last_update_time)
                    .as_secs_f64();

                if time_delta > 0.0 {
                    let bytes_delta =
                        bytes_downloaded.saturating_sub(download_info.last_bytes_downloaded) as f64;
                    download_info.download_speed = bytes_delta / time_delta;
                    download_info.last_update_time = now;
                    download_info.last_bytes_downloaded = bytes_downloaded;
                }

                download_info.bytes_downloaded = bytes_downloaded;
                if total_bytes.is_some() {
                    download_info.total_bytes = total_bytes;
                }
            }
            None
        }

        TuiEvent::NixDownloadEnd {
            operation_id: _,
            activity_id,
            success,
        } => {
            if let Some(download_info) = model.nix_downloads.get_mut(&activity_id) {
                let duration = download_info.start_time.elapsed();
                download_info.state = NixActivityState::Completed { success, duration };
            }
            // Clean up progress data
            model.activity_progress.remove(&activity_id);
            None
        }

        TuiEvent::NixQueryStart {
            operation_id,
            activity_id,
            store_path,
            package_name,
            substituter,
        } => {
            let query_info = NixQueryInfo {
                operation_id,
                activity_id,
                store_path,
                package_name,
                substituter,
                start_time: Instant::now(),
                state: NixActivityState::Active,
            };
            model.nix_queries.insert(activity_id, query_info);
            None
        }

        TuiEvent::NixQueryEnd {
            operation_id: _,
            activity_id,
            success,
        } => {
            if let Some(query_info) = model.nix_queries.get_mut(&activity_id) {
                let duration = query_info.start_time.elapsed();
                query_info.state = NixActivityState::Completed { success, duration };
            }
            // Clean up progress data
            model.activity_progress.remove(&activity_id);
            None
        }

        TuiEvent::FetchTreeStart {
            operation_id,
            activity_id,
            message,
        } => {
            let fetch_tree_info = FetchTreeInfo {
                operation_id,
                activity_id,
                message,
                start_time: Instant::now(),
                state: NixActivityState::Active,
            };
            model.fetch_trees.insert(activity_id, fetch_tree_info);
            None
        }

        TuiEvent::FetchTreeEnd {
            operation_id: _,
            activity_id,
            success,
        } => {
            if let Some(fetch_tree_info) = model.fetch_trees.get_mut(&activity_id) {
                let duration = fetch_tree_info.start_time.elapsed();
                fetch_tree_info.state = NixActivityState::Completed { success, duration };
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
            model.activity_progress.insert(
                activity_id,
                ActivityProgress {
                    done,
                    expected,
                    running,
                    failed,
                },
            );
            None
        }

        TuiEvent::Shutdown => {
            model.app_state = AppState::Shutdown;
            None
        }
    }
}
