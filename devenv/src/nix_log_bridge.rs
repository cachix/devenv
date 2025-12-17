use devenv_activity::{Activity, ActivityLevel, FetchKind, message, message_with_details};
use devenv_eval_cache::Op;
use devenv_eval_cache::internal_log::{ActivityType, Field, InternalLog, ResultType, Verbosity};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};
use tracing::{error, info, trace, warn};

/// Bridge that converts Nix internal logs to tracing events.
///
/// The bridge must be created with a parent activity ID that is captured
/// before crossing thread boundaries, since the stderr callback runs in
/// a separate thread and cannot access the task-local activity stack.
pub struct NixLogBridge {
    /// Current active operations and their associated Nix activities
    active_activities: Arc<Mutex<HashMap<u64, NixActivityInfo>>>,
    /// Parent activity ID for all activities created by this bridge
    parent_activity_id: Option<u64>,
    /// The evaluation activity for tracking file evaluations
    evaluation_activity: Arc<Mutex<Option<Activity>>>,
}

/// Information about an active Nix activity
struct NixActivityInfo {
    activity_type: ActivityType,
    activity: Activity,
}

impl NixLogBridge {
    /// Create a new NixLogBridge with the given parent activity ID.
    ///
    /// The parent_activity_id should be captured using `current_activity_id()`
    /// before spawning any threads, as the callback runs in a separate thread.
    pub fn new(parent_activity_id: Option<u64>) -> Arc<Self> {
        Arc::new(Self {
            active_activities: Arc::new(Mutex::new(HashMap::new())),
            parent_activity_id,
            evaluation_activity: Arc::new(Mutex::new(None)),
        })
    }

    /// Returns a callback that can be used by any log source.
    /// Both CLI and FFI backends can use this to feed logs to the bridge.
    ///
    /// # Example
    /// ```
    /// use devenv::nix_log_bridge::NixLogBridge;
    /// use devenv_eval_cache::internal_log::{InternalLog, ActivityType, Verbosity};
    ///
    /// let bridge = NixLogBridge::new(None);
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
                    self.process_internal_log(internal_log);
                }
                Err(e) => {
                    warn!("Failed to parse Nix internal log: {} - line: {}", e, line);
                }
            }
        }
    }

    /// Handle a parsed InternalLog entry
    fn process_internal_log(&self, log: InternalLog) {
        match log {
            InternalLog::Start {
                id,
                typ,
                text,
                fields,
                ..
            } => {
                self.handle_activity_start(id, typ, text, fields);
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
                // Note: Nix daemon incorrectly labels many routine build messages as
                // Verbosity::Error (e.g., "setting up chroot environment", "executing builder").
                // Only treat Error-level messages as actual errors if they pass is_nix_error()
                // or is_builtin_trace() checks.
                if level == Verbosity::Error {
                    if log.is_nix_error() || log.is_builtin_trace() {
                        let (summary, details) = parse_nix_error(msg);
                        message_with_details(ActivityLevel::Error, summary, details);
                        error!("{msg}");
                    }
                    // Skip falsely-labeled error messages from nix daemon
                } else if level <= Verbosity::Warn {
                    let activity_level = match level {
                        Verbosity::Warn => ActivityLevel::Warn,
                        _ => ActivityLevel::Info,
                    };
                    message(activity_level, msg);

                    // Also log to tracing for file export and non-TUI modes
                    match level {
                        Verbosity::Warn => warn!("{msg}"),
                        _ => info!("{msg}"),
                    }
                }
            }
        }
    }

    /// Insert an activity into the active activities map
    fn insert_activity(&self, activity_id: u64, activity_type: ActivityType, activity: Activity) {
        if let Ok(mut activities) = self.active_activities.lock() {
            activities.insert(
                activity_id,
                NixActivityInfo {
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

    /// Get the parent activity ID for Nix activities.
    /// Uses the evaluation activity if one exists, otherwise falls back to the
    /// parent_activity_id that was passed when creating the bridge.
    fn get_parent_activity_id(&self) -> Option<u64> {
        self.evaluation_activity
            .lock()
            .ok()
            .and_then(|eval| eval.as_ref().map(|a| a.id()))
            .or(self.parent_activity_id)
    }

    /// Handle the start of a Nix activity
    fn handle_activity_start(
        &self,
        activity_id: u64,
        activity_type: ActivityType,
        text: String,
        fields: Vec<Field>,
    ) {
        let parent_id = self.get_parent_activity_id();

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

                self.insert_activity(activity_id, activity_type, activity);
            }
            ActivityType::QueryPathInfo => {
                if let Some(store_path) = fields.first().and_then(Self::extract_string_field) {
                    let package_name = extract_package_name(&store_path);
                    let substituter = fields.get(1).and_then(Self::extract_string_field);

                    let mut builder = Activity::fetch(FetchKind::Query, package_name)
                        .id(activity_id)
                        .parent(parent_id);
                    if let Some(url) = substituter {
                        builder = builder.url(url);
                    }
                    let activity = builder.start();

                    self.insert_activity(activity_id, activity_type, activity);
                }
            }
            ActivityType::CopyPath => {
                if let Some(store_path) = fields.first().and_then(Self::extract_string_field) {
                    let package_name = extract_package_name(&store_path);
                    let substituter = fields.get(1).and_then(Self::extract_string_field);

                    let mut builder = Activity::fetch(FetchKind::Download, package_name)
                        .id(activity_id)
                        .parent(parent_id);
                    if let Some(url) = substituter {
                        builder = builder.url(url);
                    }
                    let activity = builder.start();

                    self.insert_activity(activity_id, activity_type, activity);
                }
            }
            ActivityType::FetchTree => {
                let activity = Activity::fetch(FetchKind::Tree, text)
                    .id(activity_id)
                    .parent(parent_id)
                    .start();

                self.insert_activity(activity_id, activity_type, activity);
            }
            _ => {
                trace!("Unhandled Nix activity type: {:?}", activity_type);
            }
        }
    }

    /// Handle the stop of a Nix activity
    fn handle_activity_stop(&self, activity_id: u64, success: bool) {
        let Ok(mut activities) = self.active_activities.lock() else {
            return;
        };
        let Some(activity_info) = activities.remove(&activity_id) else {
            return;
        };

        if !success {
            activity_info.activity.fail();
        }
        // Activity completes on drop
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
        if let Ok(mut eval_activity) = self.evaluation_activity.lock() {
            // If this is the first file, create the evaluation activity
            if eval_activity.is_none() {
                let activity = Activity::evaluate()
                    .parent(self.parent_activity_id)
                    .start();
                *eval_activity = Some(activity);
            }

            // Log the file path to the evaluation activity
            if let Some(ref activity) = *eval_activity {
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

/// Regex for stripping ANSI escape codes
static ANSI_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").expect("valid regex"));

/// Strip ANSI escape codes from a string
fn strip_ansi_codes(s: &str) -> String {
    ANSI_REGEX.replace_all(s, "").to_string()
}

/// Parse a Nix error message to extract the summary and details.
///
/// Nix errors have the format:
/// ```text
/// error:
///        … stack trace lines starting with ellipsis …
///        error: <actual error message>
/// ```
///
/// Returns (summary, details) where summary is the final error line
/// and details is the full original message (including stack trace).
fn parse_nix_error(msg: &str) -> (String, Option<String>) {
    // Strip ANSI codes for parsing
    let stripped = strip_ansi_codes(msg);

    // Find the last "error:" which contains the actual error
    if let Some(last_error_pos) = stripped.rfind("error:") {
        let summary = stripped[last_error_pos..].trim().to_string();

        // If there's content before the last error, include the full message as details
        let details_part = stripped[..last_error_pos].trim();
        let details = if details_part.is_empty() || details_part == "error:" {
            None
        } else {
            Some(msg.to_string()) // Keep original with ANSI codes for details
        };

        (summary, details)
    } else {
        (msg.to_string(), None)
    }
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

    #[test]
    fn test_strip_ansi_codes() {
        assert_eq!(strip_ansi_codes("\x1b[31;1merror:\x1b[0m"), "error:");
        assert_eq!(strip_ansi_codes("no codes here"), "no codes here");
        assert_eq!(
            strip_ansi_codes("\x1b[34;1mblue\x1b[0m and \x1b[32mgreen\x1b[0m"),
            "blue and green"
        );
    }

    #[test]
    fn test_parse_nix_error_simple() {
        // Simple error without stack trace
        let (summary, details) = parse_nix_error("error: attribute 'foo' not found");
        assert_eq!(summary, "error: attribute 'foo' not found");
        assert!(details.is_none());
    }

    #[test]
    fn test_parse_nix_error_with_stack_trace() {
        // Error with stack trace (like real Nix output)
        let msg = "error:\n       … while evaluating\n         at file.nix:1:1\n\n       error: undefined variable 'pkgs'";
        let (summary, details) = parse_nix_error(msg);
        assert_eq!(summary, "error: undefined variable 'pkgs'");
        assert!(details.is_some());
        assert_eq!(details.unwrap(), msg); // Original message preserved
    }

    #[test]
    fn test_parse_nix_error_with_ansi() {
        // Error with ANSI codes (like real Nix output)
        let msg = "\x1b[31;1merror:\x1b[0m\n       … stack trace\n\n       \x1b[31;1merror:\x1b[0m actual error message";
        let (summary, details) = parse_nix_error(msg);
        assert_eq!(summary, "error: actual error message");
        assert!(details.is_some());
    }

    #[test]
    fn test_parse_nix_error_only_error_prefix() {
        // Just "error:" followed by the actual message on same line
        let (summary, details) = parse_nix_error("error: something went wrong");
        assert_eq!(summary, "error: something went wrong");
        assert!(details.is_none());
    }
}
