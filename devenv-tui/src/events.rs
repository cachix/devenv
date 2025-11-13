use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Unique identifier for operations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationId(pub String);

impl OperationId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for OperationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Result of an operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationResult {
    Success,
    Failure {
        message: String,
        code: Option<i32>,
        output: Option<String>,
    },
}

/// Log levels matching tracing levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<tracing::Level> for LogLevel {
    fn from(level: tracing::Level) -> Self {
        match level {
            tracing::Level::ERROR => LogLevel::Error,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::TRACE => LogLevel::Trace,
        }
    }
}

/// Source of log messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogSource {
    User,    // Regular user-facing messages
    Tracing, // From tracing framework
    Nix,     // From Nix build logs
    System,  // System messages
}

/// State of an operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationState {
    Active,
    Complete { duration: Duration, success: bool },
}

/// Information about an operation
#[derive(Debug, Clone)]
pub struct Operation {
    pub id: OperationId,
    pub message: String,
    pub state: OperationState,
    pub start_time: Instant,
    pub children: Vec<OperationId>,
    pub parent: Option<OperationId>,
    pub data: HashMap<String, String>,
}

impl Operation {
    pub fn new(
        id: OperationId,
        message: String,
        parent: Option<OperationId>,
        data: HashMap<String, String>,
    ) -> Self {
        Self {
            id,
            message,
            state: OperationState::Active,
            start_time: Instant::now(),
            children: Vec::new(),
            parent,
            data,
        }
    }

    pub fn complete(&mut self, success: bool) {
        let duration = self.start_time.elapsed();
        self.state = OperationState::Complete { duration, success };
    }
}

/// A log message with context
#[derive(Debug, Clone)]
pub struct LogMessage {
    pub level: LogLevel,
    pub message: String,
    pub source: LogSource,
    pub timestamp: Instant,
    pub data: HashMap<String, String>,
}

impl LogMessage {
    pub fn new(
        level: LogLevel,
        message: String,
        source: LogSource,
        data: HashMap<String, String>,
    ) -> Self {
        Self {
            level,
            message,
            source,
            timestamp: Instant::now(),
            data,
        }
    }
}

/// Information about a Nix build
#[derive(Debug, Clone)]
pub struct NixBuildInfo {
    pub operation_id: OperationId,
    pub derivation: String,
    pub current_phase: Option<String>,
    pub start_time: Instant,
}

/// Information about a Nix derivation being built
#[derive(Debug, Clone)]
pub struct NixDerivationInfo {
    pub operation_id: OperationId,
    pub activity_id: u64,
    pub derivation_path: String,
    pub derivation_name: String,
    pub machine: Option<String>,
    pub current_phase: Option<String>,
    pub start_time: Instant,
    pub state: NixActivityState,
}

/// Information about a Nix download
#[derive(Debug, Clone)]
pub struct NixDownloadInfo {
    pub operation_id: OperationId,
    pub activity_id: u64,
    pub store_path: String,
    pub package_name: String,
    pub substituter: String,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub start_time: Instant,
    pub state: NixActivityState,
    pub last_update_time: Instant,
    pub last_bytes_downloaded: u64,
    pub download_speed: f64, // bytes per second
}

/// Information about a Nix store query
#[derive(Debug, Clone)]
pub struct NixQueryInfo {
    pub operation_id: OperationId,
    pub activity_id: u64,
    pub store_path: String,
    pub package_name: String,
    pub substituter: String,
    pub start_time: Instant,
    pub state: NixActivityState,
}

/// Information about a fetch tree activity
#[derive(Debug, Clone)]
pub struct FetchTreeInfo {
    pub operation_id: OperationId,
    pub activity_id: u64,
    pub message: String,
    pub start_time: Instant,
    pub state: NixActivityState,
}

/// State of a Nix activity
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NixActivityState {
    Active,
    Completed { success: bool, duration: Duration },
}

/// Type of Nix activity for categorization
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NixActivityType {
    Build,
    Download,
    Query,
    Evaluating,
    FetchTree,
    Unknown,
    UserOperation,
}

impl std::fmt::Display for NixActivityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NixActivityType::Build => write!(f, "building"),
            NixActivityType::Download => write!(f, "downloading"),
            NixActivityType::Query => write!(f, "querying"),
            NixActivityType::Evaluating => write!(f, "evaluating"),
            NixActivityType::FetchTree => write!(f, "fetching"),
            NixActivityType::Unknown => write!(f, "unknown"),
            NixActivityType::UserOperation => write!(f, ""), // No prefix for user operations
        }
    }
}

/// Progress information for an activity
#[derive(Debug, Clone)]
pub struct ActivityProgress {
    pub done: u64,
    pub expected: u64,
    pub running: u64,
    pub failed: u64,
}

/// Typed tracing updates that the model understands
#[derive(Debug, Clone)]
pub enum TracingUpdate {
    NixProgress(NixProgressUpdate),
    BuildPhase(BuildPhaseUpdate),
    BuildLog(BuildLogUpdate),
    DownloadProgress(DownloadProgressUpdate),
    EvaluationProgress(EvaluationProgressUpdate),
    TaskStatus(TaskStatusUpdate),
    LogOutput(LogOutputUpdate),
}

/// Nix progress update (for builds, copies, etc.)
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NixProgressUpdate {
    pub activity_id: u64,
    pub done: u64,
    pub expected: u64,
    #[serde(default)]
    pub running: u64,
    #[serde(default)]
    pub failed: u64,
}

/// Build phase update
#[derive(Debug, Clone, serde::Deserialize)]
pub struct BuildPhaseUpdate {
    pub activity_id: u64,
    pub phase: String,
}

/// Build log line
#[derive(Debug, Clone, serde::Deserialize)]
pub struct BuildLogUpdate {
    pub activity_id: u64,
    pub line: String,
}

/// Download progress update
#[derive(Debug, Clone, serde::Deserialize)]
pub struct DownloadProgressUpdate {
    pub activity_id: u64,
    pub bytes_downloaded: u64,
    #[serde(default)]
    pub total_bytes: Option<u64>,
}

/// Evaluation progress update
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EvaluationProgressUpdate {
    pub activity_id: u64,
    #[serde(default)]
    pub total_files_evaluated: u64,
    #[serde(rename = "files", deserialize_with = "deserialize_files_array")]
    pub latest_files: Vec<String>,
}

/// Custom deserializer for the files array which might be a string like '["file1", "file2"]'
/// or already parsed as a Vec
fn deserialize_files_array<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    use serde_json::Value;

    let value = Value::deserialize(deserializer)?;

    match value {
        // If it's already an array (from our conversion), use it directly
        Value::Array(arr) => {
            Ok(arr.into_iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        }
        // If it's a string containing JSON array, parse it
        Value::String(s) => {
            serde_json::from_str::<Vec<String>>(&s).or_else(|_| Ok(Vec::new()))
        }
        _ => Ok(Vec::new()),
    }
}

/// Task status update
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TaskStatusUpdate {
    #[serde(alias = "devenv.ui.task.name")]
    pub name: String,
    #[serde(alias = "devenv.ui.status", default = "default_status")]
    pub status: String,
    #[serde(alias = "devenv.ui.status.result")]
    pub result: Option<String>,
    pub duration_secs: Option<f64>,
    pub success: Option<bool>,
    pub error: Option<String>,
}

fn default_status() -> String {
    "unknown".to_string()
}

/// Log output (stdout/stderr)
#[derive(Debug, Clone, serde::Deserialize)]
pub struct LogOutputUpdate {
    #[serde(alias = "devenv.ui.log.stream")]
    pub stream: String,
    #[serde(alias = "devenv.ui.log.message")]
    pub message: String,
}

/// Helper to deserialize from HashMap<String, String> using serde
fn from_fields<T: serde::de::DeserializeOwned>(
    fields: &std::collections::HashMap<String, String>,
) -> Option<T> {
    // Convert HashMap<String, String> to serde_json::Value
    let map: serde_json::Map<String, serde_json::Value> = fields
        .iter()
        .map(|(k, v)| {
            // Try to parse as JSON value first (for bools, numbers, arrays)
            let value = serde_json::from_str(v).unwrap_or_else(|_| {
                // If it fails, treat as string
                serde_json::Value::String(v.clone())
            });
            (k.clone(), value)
        })
        .collect();

    let value = serde_json::Value::Object(map);

    // Deserialize using serde
    serde_json::from_value(value).ok()
}

impl NixProgressUpdate {
    pub fn from_fields(fields: &std::collections::HashMap<String, String>) -> Option<Self> {
        from_fields(fields)
    }
}

impl BuildPhaseUpdate {
    pub fn from_fields(fields: &std::collections::HashMap<String, String>) -> Option<Self> {
        from_fields(fields)
    }
}

impl BuildLogUpdate {
    pub fn from_fields(fields: &std::collections::HashMap<String, String>) -> Option<Self> {
        from_fields(fields)
    }
}

impl DownloadProgressUpdate {
    pub fn from_fields(fields: &std::collections::HashMap<String, String>) -> Option<Self> {
        from_fields(fields)
    }
}

impl EvaluationProgressUpdate {
    pub fn from_fields(fields: &std::collections::HashMap<String, String>) -> Option<Self> {
        from_fields(fields)
    }
}

impl TaskStatusUpdate {
    pub fn from_fields(fields: &std::collections::HashMap<String, String>) -> Option<Self> {
        from_fields(fields)
    }
}

impl LogOutputUpdate {
    pub fn from_fields(fields: &std::collections::HashMap<String, String>) -> Option<Self> {
        from_fields(fields)
    }
}

impl TracingUpdate {
    /// Parse a tracing event into a typed update
    pub fn from_event(
        target: &str,
        event_name: &str,
        fields: &std::collections::HashMap<String, String>,
    ) -> Option<Self> {
        match (target, event_name) {
            ("devenv.nix.progress", _) => {
                NixProgressUpdate::from_fields(fields).map(TracingUpdate::NixProgress)
            }
            ("devenv.nix.build", _) if fields.contains_key("phase") => {
                BuildPhaseUpdate::from_fields(fields).map(TracingUpdate::BuildPhase)
            }
            ("devenv.nix.build", _) if fields.contains_key("line") => {
                BuildLogUpdate::from_fields(fields).map(TracingUpdate::BuildLog)
            }
            ("devenv.nix.download", "nix_download_progress") => {
                DownloadProgressUpdate::from_fields(fields).map(TracingUpdate::DownloadProgress)
            }
            ("devenv.nix.eval", "nix_evaluation_progress") => {
                EvaluationProgressUpdate::from_fields(fields).map(TracingUpdate::EvaluationProgress)
            }
            (t, _) if t.starts_with("devenv_tasks") => {
                TaskStatusUpdate::from_fields(fields).map(TracingUpdate::TaskStatus)
            }
            ("stdout", _) | ("stderr", _) => {
                LogOutputUpdate::from_fields(fields).map(TracingUpdate::LogOutput)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create field map from key-value pairs
    fn fields(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_parse_nix_progress_valid() {
        let fields = fields(&[
            ("activity_id", "42"),
            ("done", "10"),
            ("expected", "100"),
            ("running", "2"),
            ("failed", "1"),
        ]);

        let update = NixProgressUpdate::from_fields(&fields).unwrap();

        assert_eq!(update.activity_id, 42);
        assert_eq!(update.done, 10);
        assert_eq!(update.expected, 100);
        assert_eq!(update.running, 2);
        assert_eq!(update.failed, 1);
    }

    #[test]
    fn test_parse_nix_progress_minimal() {
        let fields = fields(&[
            ("activity_id", "42"),
            ("done", "10"),
            ("expected", "100"),
        ]);

        let update = NixProgressUpdate::from_fields(&fields).unwrap();

        assert_eq!(update.activity_id, 42);
        assert_eq!(update.done, 10);
        assert_eq!(update.expected, 100);
        assert_eq!(update.running, 0); // default
        assert_eq!(update.failed, 0); // default
    }

    #[test]
    fn test_parse_nix_progress_missing_required() {
        let fields = fields(&[
            ("activity_id", "42"),
            ("done", "10"),
        ]);

        let update = NixProgressUpdate::from_fields(&fields);
        assert!(update.is_none());
    }

    #[test]
    fn test_parse_nix_progress_invalid_number() {
        let fields = fields(&[
            ("activity_id", "not_a_number"),
            ("done", "10"),
            ("expected", "100"),
        ]);

        let update = NixProgressUpdate::from_fields(&fields);
        assert!(update.is_none());
    }

    #[test]
    fn test_parse_build_phase_valid() {
        let fields = fields(&[
            ("activity_id", "42"),
            ("phase", "configure"),
        ]);

        let update = BuildPhaseUpdate::from_fields(&fields).unwrap();

        assert_eq!(update.activity_id, 42);
        assert_eq!(update.phase, "configure");
    }

    #[test]
    fn test_parse_download_progress_with_bool_success() {
        // Test that serde properly deserializes booleans
        let fields = fields(&[
            ("activity_id", "42"),
            ("bytes_downloaded", "1024"),
            ("total_bytes", "4096"),
        ]);

        let update = DownloadProgressUpdate::from_fields(&fields).unwrap();

        assert_eq!(update.activity_id, 42);
        assert_eq!(update.bytes_downloaded, 1024);
        assert_eq!(update.total_bytes, Some(4096));
    }

    #[test]
    fn test_parse_task_status_with_serde_bool() {
        // Test that serde properly handles bool parsing
        let fields = fields(&[
            ("name", "build:hello"),
            ("status", "completed"),
            ("success", "true"), // serde will parse this
            ("duration_secs", "1.5"),
        ]);

        let update = TaskStatusUpdate::from_fields(&fields).unwrap();

        assert_eq!(update.name, "build:hello");
        assert_eq!(update.status, "completed");
        assert_eq!(update.success, Some(true));
        assert_eq!(update.duration_secs, Some(1.5));
    }

    #[test]
    fn test_parse_task_status_with_quoted_bool() {
        // Test that serde handles JSON-style quoted values
        let fields = fields(&[
            ("name", "build:fail"),
            ("status", "\"failed\""),
            ("success", "false"),
            ("error", "\"build error\""),
        ]);

        let update = TaskStatusUpdate::from_fields(&fields).unwrap();

        assert_eq!(update.name, "build:fail");
        assert_eq!(update.status, "failed");
        assert_eq!(update.success, Some(false));
        assert_eq!(update.error, Some("build error".to_string()));
    }

    #[test]
    fn test_parse_evaluation_progress_with_json_array() {
        let fields = fields(&[
            ("activity_id", "42"),
            ("total_files_evaluated", "150"),
            ("files", r#"["file1.nix", "file2.nix", "file3.nix"]"#),
        ]);

        let update = EvaluationProgressUpdate::from_fields(&fields).unwrap();

        assert_eq!(update.activity_id, 42);
        assert_eq!(update.total_files_evaluated, 150);
        assert_eq!(update.latest_files, vec!["file1.nix", "file2.nix", "file3.nix"]);
    }

    #[test]
    fn test_tracing_update_from_event_nix_progress() {
        let fields = fields(&[
            ("activity_id", "42"),
            ("done", "50"),
            ("expected", "100"),
        ]);

        let update = TracingUpdate::from_event("devenv.nix.progress", "progress", &fields);

        assert!(matches!(update, Some(TracingUpdate::NixProgress(_))));
    }

    #[test]
    fn test_tracing_update_from_event_invalid_fields() {
        let fields = fields(&[("activity_id", "not_a_number")]);

        let update = TracingUpdate::from_event("devenv.nix.progress", "progress", &fields);

        assert!(update.is_none());
    }

    #[test]
    fn test_serde_handles_field_aliases() {
        // Test that serde alias attributes work
        let fields = fields(&[
            ("devenv.ui.task.name", "mytask"),
            ("devenv.ui.status", "running"),
        ]);

        let update = TaskStatusUpdate::from_fields(&fields).unwrap();

        assert_eq!(update.name, "mytask");
        assert_eq!(update.status, "running");
    }

    #[test]
    fn test_serde_uses_defaults() {
        // Test that serde default attributes work
        let fields = fields(&[
            ("activity_id", "42"),
            ("bytes_downloaded", "1024"),
            // total_bytes omitted - should default to None
        ]);

        let update = DownloadProgressUpdate::from_fields(&fields).unwrap();

        assert_eq!(update.total_bytes, None);
    }
}
