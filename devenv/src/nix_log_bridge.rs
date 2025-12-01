use devenv_activity::{Activity, FetchKind};
use devenv_eval_cache::Op;
use devenv_eval_cache::internal_log::{ActivityType, Field, InternalLog, ResultType, Verbosity};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{error, info, trace, warn};

/// Simple operation ID type for correlating Nix activities
pub type OperationId = String;

/// Bridge that converts Nix internal logs to tracing events
pub struct NixLogBridge {
    /// Current active operations and their associated Nix activities
    active_activities: Arc<Mutex<HashMap<u64, NixActivityInfo>>>,
    /// Current parent operation ID for correlating Nix activities
    current_operation_id: Arc<Mutex<Option<OperationId>>>,
    /// Evaluation tracking state
    evaluation_state: Arc<Mutex<EvaluationState>>,
}

/// State for tracking file evaluations
#[derive(Default)]
struct EvaluationState {
    /// The activity tracking the entire evaluation operation
    activity: Option<Activity>,
}

/// Information about an active Nix activity
struct NixActivityInfo {
    #[allow(dead_code)]
    operation_id: OperationId,
    activity_type: ActivityType,
    activity: Activity,
}

impl NixLogBridge {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            active_activities: Arc::new(Mutex::new(HashMap::new())),
            current_operation_id: Arc::new(Mutex::new(None)),
            evaluation_state: Arc::new(Mutex::new(EvaluationState::default())),
        })
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

        // Reset the evaluation state for the next operation
        if let Ok(mut state) = self.evaluation_state.lock() {
            state.activity = None; // Drop the activity to end evaluation
        }
    }

    /// Returns a callback that can be used by any log source.
    /// Both CLI and FFI backends can use this to feed logs to the bridge.
    ///
    /// # Example
    /// ```
    /// use devenv::nix_log_bridge::NixLogBridge;
    /// use devenv_eval_cache::internal_log::{InternalLog, ActivityType, Verbosity};
    ///
    /// let bridge = NixLogBridge::new();
    /// let callback = bridge.get_log_callback();
    ///
    /// // Feed logs from any source
    /// callback(InternalLog::Start {
    ///     id: 1,
    ///     typ: ActivityType::Unknown,
    ///     text: "example".to_string(),
    ///     fields: vec![],
    ///     level: Verbosity::Error,
    ///     parent: 0,
    /// });
    /// ```
    pub fn get_log_callback(
        self: &Arc<Self>,
    ) -> impl Fn(InternalLog) + Clone + Send + Sync + 'static {
        let bridge = Arc::clone(self);
        move |log: InternalLog| {
            bridge.process_internal_log(log);
        }
    }

    /// Process a Nix internal log line and emit appropriate tracing events
    pub fn process_log_line(&self, line: &str) {
        if let Some(parse_result) = InternalLog::parse(line) {
            match parse_result {
                Ok(internal_log) => {
                    self.handle_internal_log(internal_log);
                }
                Err(e) => {
                    warn!("Failed to parse Nix internal log: {} - line: {}", e, line);
                }
            }
        }
    }

    /// Process a parsed InternalLog directly
    pub fn process_internal_log(&self, log: InternalLog) {
        self.handle_internal_log(log);
    }

    /// Process stderr from a pipe, reading line by line and feeding to the bridge
    pub fn process_stderr<R: std::io::Read>(
        &self,
        stderr: R,
        logging: bool,
    ) -> std::io::Result<()> {
        use std::io::{BufRead, BufReader};

        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let line = line?;

            // Feed line to bridge for structured log processing
            self.process_log_line(&line);

            // Also output to terminal if logging is enabled
            if logging {
                eprintln!("{}", line);
            }
        }

        Ok(())
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
                    if let Ok(activities) = self.active_activities.lock()
                        && let Some((_, activity_info)) = activities
                            .iter()
                            .find(|(_, info)| info.activity_type == ActivityType::Build)
                    {
                        activity_info.activity.phase(&phase);
                    }
                }
                InternalLog::Msg { level, ref msg, .. } => {
                    // First check if this is a file evaluation message
                    if let Some(op) = Op::from_internal_log(&log)
                        && let Op::EvaluatedFile { source } = op
                    {
                        self.handle_file_evaluation(source);
                        return;
                    }

                    // Handle regular log messages from Nix builds
                    if level <= Verbosity::Warn {
                        match level {
                            Verbosity::Error => error!("{msg}"),
                            Verbosity::Warn => warn!("{msg}"),
                            _ => info!("{msg}"),
                        }
                    }
                }
            }
        }
    }

    /// Insert an activity into the active activities map
    fn insert_activity(
        &self,
        activity_id: u64,
        operation_id: OperationId,
        activity_type: ActivityType,
        activity: Activity,
    ) {
        if let Ok(mut activities) = self.active_activities.lock() {
            activities.insert(
                activity_id,
                NixActivityInfo {
                    operation_id,
                    activity_type,
                    activity,
                },
            );
        }
    }

    /// Extract a string value from a Field
    fn extract_string_field(field: &Field) -> Option<String> {
        match field {
            Field::String(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// Get the current evaluation activity ID if one exists
    fn get_evaluation_activity_id(&self) -> Option<u64> {
        self.evaluation_state
            .lock()
            .ok()
            .and_then(|state| state.activity.as_ref().map(|a| a.id()))
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
        // Get the evaluation activity ID to use as parent for all Nix activities.
        // This ensures parallel queries/downloads are children of the evaluation,
        // not children of each other.
        let parent_id = self.get_evaluation_activity_id();

        match activity_type {
            ActivityType::Build => {
                let derivation_path = fields
                    .first()
                    .and_then(Self::extract_string_field)
                    .unwrap_or_else(|| text.clone());

                let derivation_name = extract_derivation_name(&derivation_path);

                let activity = Activity::build(derivation_name)
                    .id(activity_id)
                    .derivation_path(derivation_path)
                    .parent(parent_id)
                    .start();

                self.insert_activity(activity_id, operation_id, activity_type, activity);
            }
            ActivityType::QueryPathInfo => {
                if let Some(store_path) = fields.first().and_then(Self::extract_string_field) {
                    let package_name = extract_package_name(&store_path);

                    let activity =
                        Activity::fetch(FetchKind::Query, format!("Query {}", package_name))
                            .id(activity_id)
                            .url(store_path)
                            .parent(parent_id)
                            .start();

                    self.insert_activity(activity_id, operation_id, activity_type, activity);
                }
            }
            ActivityType::CopyPath => {
                if let Some(store_path) = fields.first().and_then(Self::extract_string_field) {
                    let package_name = extract_package_name(&store_path);

                    let activity = Activity::fetch(FetchKind::Download, package_name)
                        .id(activity_id)
                        .url(store_path)
                        .parent(parent_id)
                        .start();

                    self.insert_activity(activity_id, operation_id, activity_type, activity);
                }
            }
            ActivityType::FetchTree => {
                let activity = Activity::fetch(FetchKind::Tree, text)
                    .id(activity_id)
                    .parent(parent_id)
                    .start();

                self.insert_activity(activity_id, operation_id, activity_type, activity);
            }
            _ => {
                trace!("Unhandled Nix activity type: {:?}", activity_type);
            }
        }
    }

    /// Handle the stop of a Nix activity
    fn handle_activity_stop(&self, activity_id: u64, success: bool) {
        if let Ok(mut activities) = self.active_activities.lock()
            && let Some(activity_info) = activities.remove(&activity_id)
        {
            // Mark as failed if not successful, then drop to complete
            if !success {
                activity_info.activity.fail();
            }
            // Activity completes on drop
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
                // Handle generic progress updates with format [done, expected, running, failed]
                if fields.len() >= 4 {
                    if let (Some(Field::Int(done)), Some(Field::Int(expected)), _, _) =
                        (fields.first(), fields.get(1), fields.get(2), fields.get(3))
                        && let Ok(activities) = self.active_activities.lock()
                        && let Some(activity_info) = activities.get(&activity_id)
                    {
                        activity_info.activity.progress(*done, *expected);
                    }
                } else if fields.len() >= 2 {
                    // Fallback to download progress format for backward compatibility
                    if let (Some(Field::Int(downloaded)), total_opt) =
                        (fields.first(), fields.get(1))
                    {
                        let total_bytes = match total_opt {
                            Some(Field::Int(total)) => Some(*total),
                            _ => None,
                        };

                        if let Ok(activities) = self.active_activities.lock()
                            && let Some(activity_info) = activities.get(&activity_id)
                        {
                            // Only CopyPath activities have byte-based download progress
                            if activity_info.activity_type == ActivityType::CopyPath {
                                if let Some(total) = total_bytes {
                                    activity_info.activity.progress_bytes(*downloaded, total);
                                } else {
                                    activity_info.activity.progress_indeterminate(*downloaded);
                                }
                            }
                        }
                    }
                }
            }
            ResultType::SetPhase => {
                // Handle build phase changes
                if let Some(Field::String(phase)) = fields.first()
                    && let Ok(activities) = self.active_activities.lock()
                    && let Some(activity_info) = activities.get(&activity_id)
                    && activity_info.activity_type == ActivityType::Build
                {
                    activity_info.activity.phase(phase);
                }
            }
            ResultType::BuildLogLine => {
                // Handle build log output
                if let Some(Field::String(log_line)) = fields.first()
                    && let Ok(activities) = self.active_activities.lock()
                    && let Some(activity_info) = activities.get(&activity_id)
                {
                    activity_info.activity.log(log_line);
                }
            }
            _ => {
                trace!("Unhandled Nix result type: {:?}", result_type);
            }
        }
    }

    /// Handle file evaluation events
    fn handle_file_evaluation(&self, file_path: std::path::PathBuf) {
        if let Ok(mut state) = self.evaluation_state.lock() {
            // If this is the first file, create the evaluation activity
            if state.activity.is_none() {
                let activity = Activity::evaluate("").start();
                state.activity = Some(activity);
            }

            // Log the file path to the evaluation activity
            if let Some(ref activity) = state.activity {
                activity.log(file_path.display().to_string());
            }
        }
    }
}

/// Extract a human-readable name from a Nix path
///
/// For derivations, strips .drv suffix if present.
/// Extracts the name part after the hash (format: /nix/store/hash-name)
fn extract_nix_name(path: &str, strip_drv: bool) -> String {
    // Remove .drv suffix if requested
    let path = if strip_drv {
        path.strip_suffix(".drv").unwrap_or(path)
    } else {
        path
    };

    // Extract the name part after the hash
    if let Some(dash_pos) = path.rfind('-')
        && let Some(slash_pos) = path[..dash_pos].rfind('/')
    {
        return path[slash_pos + 1..].to_string();
    }

    // Fallback: just take the filename
    path.split('/').next_back().unwrap_or(path).to_string()
}

/// Extract a human-readable derivation name from a derivation path
fn extract_derivation_name(derivation_path: &str) -> String {
    extract_nix_name(derivation_path, true)
}

/// Extract a human-readable package name from a store path
fn extract_package_name(store_path: &str) -> String {
    extract_nix_name(store_path, false)
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
