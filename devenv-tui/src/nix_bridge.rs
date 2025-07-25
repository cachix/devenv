use crate::{OperationId, TuiEvent};
use devenv_eval_cache::internal_log::{ActivityType, Field, InternalLog, ResultType, Verbosity};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Bridge that converts Nix internal logs to TUI events
pub struct NixLogBridge {
    /// Channel to send TUI events
    tui_sender: mpsc::UnboundedSender<TuiEvent>,
    /// Current active operations and their associated Nix activities
    active_activities: Arc<Mutex<HashMap<u64, NixActivityInfo>>>,
    /// Current parent operation ID for correlating Nix activities
    current_operation_id: Arc<Mutex<Option<OperationId>>>,
}

/// Information about an active Nix activity
#[derive(Debug, Clone)]
struct NixActivityInfo {
    operation_id: OperationId,
    activity_type: ActivityType,
    start_time: std::time::Instant,
}

impl NixLogBridge {
    pub fn new(tui_sender: mpsc::UnboundedSender<TuiEvent>) -> Self {
        Self {
            tui_sender,
            active_activities: Arc::new(Mutex::new(HashMap::new())),
            current_operation_id: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the current operation ID for correlating Nix activities
    pub fn set_current_operation(&self, operation_id: OperationId) {
        if let Ok(mut current) = self.current_operation_id.lock() {
            *current = Some(operation_id);
        }
    }

    /// Clear the current operation ID
    pub fn clear_current_operation(&self) {
        if let Ok(mut current) = self.current_operation_id.lock() {
            *current = None;
        }
    }

    /// Process a Nix internal log line and emit appropriate TUI events
    pub fn process_log_line(&self, line: &str) {
        if let Some(parse_result) = InternalLog::parse(line) {
            match parse_result {
                Ok(internal_log) => {
                    self.handle_internal_log(internal_log);
                }
                Err(e) => {
                    tracing::debug!("Failed to parse Nix internal log: {} - line: {}", e, line);
                }
            }
        }
    }

    /// Process a parsed InternalLog directly
    pub fn process_internal_log(&self, log: InternalLog) {
        self.handle_internal_log(log);
    }

    /// Handle a parsed InternalLog entry
    fn handle_internal_log(&self, log: InternalLog) {
        let current_op_id = self
            .current_operation_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone());

        if let Some(operation_id) = current_op_id {
            match log {
                InternalLog::Start {
                    id,
                    typ,
                    text,
                    fields,
                    ..
                } => {
                    self.handle_activity_start(operation_id, id, typ, text, fields);
                }
                InternalLog::Stop { id } => {
                    self.handle_activity_stop(id, true);
                }
                InternalLog::Result { id, typ, fields } => {
                    self.handle_activity_result(id, typ, fields);
                }
                InternalLog::SetPhase { phase } => {
                    // Find the most recent build activity and update its phase
                    if let Ok(activities) = self.active_activities.lock() {
                        if let Some((activity_id, _)) = activities
                            .iter()
                            .find(|(_, info)| info.activity_type == ActivityType::Build)
                        {
                            let _ = self.tui_sender.send(TuiEvent::NixPhaseProgress {
                                operation_id: operation_id.clone(),
                                activity_id: *activity_id,
                                phase,
                            });
                        }
                    }
                }
                InternalLog::Msg { level, msg, .. } => {
                    // Handle log messages from Nix builds
                    if level <= Verbosity::Warn {
                        let _ = self.tui_sender.send(TuiEvent::LogMessage {
                            level: match level {
                                Verbosity::Error => crate::LogLevel::Error,
                                Verbosity::Warn => crate::LogLevel::Warn,
                                _ => crate::LogLevel::Info,
                            },
                            message: msg,
                            source: crate::LogSource::Nix,
                            data: HashMap::new(),
                        });
                    }
                }
            }
        }
    }

    /// Handle the start of a Nix activity
    fn handle_activity_start(
        &self,
        operation_id: OperationId,
        activity_id: u64,
        activity_type: ActivityType,
        text: String,
        fields: Vec<Field>,
    ) {
        // Store activity info for later correlation
        if let Ok(mut activities) = self.active_activities.lock() {
            activities.insert(
                activity_id,
                NixActivityInfo {
                    operation_id: operation_id.clone(),
                    activity_type,
                    start_time: std::time::Instant::now(),
                },
            );
        }

        match activity_type {
            ActivityType::Build => {
                let derivation_path = fields
                    .get(0)
                    .and_then(|f| match f {
                        Field::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| text.clone());

                let machine = fields.get(1).and_then(|f| match f {
                    Field::String(s) => Some(s.clone()),
                    _ => None,
                });

                let derivation_name = extract_derivation_name(&derivation_path);

                let _ = self.tui_sender.send(TuiEvent::NixDerivationStart {
                    operation_id,
                    activity_id,
                    derivation_path,
                    derivation_name,
                    machine,
                });
            }
            ActivityType::Substitute => {
                if let (Some(Field::String(store_path)), Some(Field::String(substituter))) =
                    (fields.get(0), fields.get(1))
                {
                    let package_name = extract_package_name(store_path);

                    let _ = self.tui_sender.send(TuiEvent::NixDownloadStart {
                        operation_id,
                        activity_id,
                        store_path: store_path.clone(),
                        package_name,
                        substituter: substituter.clone(),
                    });
                }
            }
            ActivityType::QueryPathInfo => {
                if let (Some(Field::String(store_path)), Some(Field::String(substituter))) =
                    (fields.get(0), fields.get(1))
                {
                    let package_name = extract_package_name(store_path);

                    let _ = self.tui_sender.send(TuiEvent::NixQueryStart {
                        operation_id,
                        activity_id,
                        store_path: store_path.clone(),
                        package_name,
                        substituter: substituter.clone(),
                    });
                }
            }
            _ => {
                // For other activity types, we can add support as needed
                tracing::debug!("Unhandled Nix activity type: {:?}", activity_type);
            }
        }
    }

    /// Handle the stop of a Nix activity
    fn handle_activity_stop(&self, activity_id: u64, success: bool) {
        if let Ok(mut activities) = self.active_activities.lock() {
            if let Some(activity_info) = activities.remove(&activity_id) {
                match activity_info.activity_type {
                    ActivityType::Build => {
                        let _ = self.tui_sender.send(TuiEvent::NixDerivationEnd {
                            operation_id: activity_info.operation_id,
                            activity_id,
                            success,
                        });
                    }
                    ActivityType::Substitute => {
                        let _ = self.tui_sender.send(TuiEvent::NixDownloadEnd {
                            operation_id: activity_info.operation_id,
                            activity_id,
                            success,
                        });
                    }
                    ActivityType::QueryPathInfo => {
                        let _ = self.tui_sender.send(TuiEvent::NixQueryEnd {
                            operation_id: activity_info.operation_id,
                            activity_id,
                            success,
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    /// Handle activity result messages (like progress updates)
    fn handle_activity_result(
        &self,
        activity_id: u64,
        result_type: ResultType,
        fields: Vec<Field>,
    ) {
        match result_type {
            ResultType::Progress => {
                // Handle download progress updates
                if let (Some(Field::Int(downloaded)), total_opt) = (fields.get(0), fields.get(1)) {
                    let total_bytes = match total_opt {
                        Some(Field::Int(total)) => Some(*total),
                        _ => None,
                    };

                    if let Ok(activities) = self.active_activities.lock() {
                        if let Some(activity_info) = activities.get(&activity_id) {
                            if activity_info.activity_type == ActivityType::Substitute {
                                let _ = self.tui_sender.send(TuiEvent::NixDownloadProgress {
                                    operation_id: activity_info.operation_id.clone(),
                                    activity_id,
                                    bytes_downloaded: *downloaded,
                                    total_bytes,
                                });
                            }
                        }
                    }
                }
            }
            ResultType::SetPhase => {
                // Handle build phase changes
                if let Some(Field::String(phase)) = fields.get(0) {
                    if let Ok(activities) = self.active_activities.lock() {
                        if let Some(activity_info) = activities.get(&activity_id) {
                            if activity_info.activity_type == ActivityType::Build {
                                let _ = self.tui_sender.send(TuiEvent::NixPhaseProgress {
                                    operation_id: activity_info.operation_id.clone(),
                                    activity_id,
                                    phase: phase.clone(),
                                });
                            }
                        }
                    }
                }
            }
            ResultType::BuildLogLine => {
                // Handle build log output
                if let Some(Field::String(log_line)) = fields.get(0) {
                    if let Ok(activities) = self.active_activities.lock() {
                        if let Some(activity_info) = activities.get(&activity_id) {
                            let _ = self.tui_sender.send(TuiEvent::LogMessage {
                                level: crate::LogLevel::Info,
                                message: log_line.clone(),
                                source: crate::LogSource::Nix,
                                data: HashMap::new(),
                            });
                        }
                    }
                }
            }
            _ => {
                // Handle other result types as needed
                tracing::debug!("Unhandled Nix result type: {:?}", result_type);
            }
        }
    }
}

/// Extract a human-readable derivation name from a derivation path
fn extract_derivation_name(derivation_path: &str) -> String {
    // Remove .drv suffix if present
    let path = derivation_path
        .strip_suffix(".drv")
        .unwrap_or(derivation_path);

    // Extract the name part after the hash
    if let Some(dash_pos) = path.rfind('-') {
        if let Some(slash_pos) = path[..dash_pos].rfind('/') {
            return path[slash_pos + 1..].to_string();
        }
    }

    // Fallback: just take the filename
    path.split('/').last().unwrap_or(path).to_string()
}

/// Extract a human-readable package name from a store path
fn extract_package_name(store_path: &str) -> String {
    // Extract the name part after the hash (format: /nix/store/hash-name)
    if let Some(dash_pos) = store_path.rfind('-') {
        if let Some(slash_pos) = store_path[..dash_pos].rfind('/') {
            return store_path[slash_pos + 1..].to_string();
        }
    }

    // Fallback: just take the filename
    store_path
        .split('/')
        .last()
        .unwrap_or(store_path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_derivation_name() {
        assert_eq!(
            extract_derivation_name("/nix/store/abc123-hello-world-1.0.drv"),
            "abc123-hello-world-1.0"
        );
        assert_eq!(
            extract_derivation_name("/nix/store/xyz456-rust-1.70.0"),
            "xyz456-rust-1.70.0"
        );
        assert_eq!(extract_derivation_name("simple-name.drv"), "simple-name");
    }

    #[test]
    fn test_extract_package_name() {
        assert_eq!(
            extract_package_name("/nix/store/abc123-hello-world-1.0"),
            "abc123-hello-world-1.0"
        );
        assert_eq!(
            extract_package_name("/nix/store/xyz456-rust-1.70.0-dev"),
            "xyz456-rust-1.70.0-dev"
        );
        assert_eq!(extract_package_name("simple-name"), "simple-name");
    }
}
