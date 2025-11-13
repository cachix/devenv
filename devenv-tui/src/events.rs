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
#[derive(Debug, Clone)]
pub struct NixProgressUpdate {
    pub activity_id: u64,
    pub done: u64,
    pub expected: u64,
    pub running: u64,
    pub failed: u64,
}

/// Build phase update
#[derive(Debug, Clone)]
pub struct BuildPhaseUpdate {
    pub activity_id: u64,
    pub phase: String,
}

/// Build log line
#[derive(Debug, Clone)]
pub struct BuildLogUpdate {
    pub activity_id: u64,
    pub line: String,
}

/// Download progress update
#[derive(Debug, Clone)]
pub struct DownloadProgressUpdate {
    pub activity_id: u64,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
}

/// Evaluation progress update
#[derive(Debug, Clone)]
pub struct EvaluationProgressUpdate {
    pub activity_id: u64,
    pub total_files_evaluated: u64,
    pub latest_files: Vec<String>,
}

/// Task status update
#[derive(Debug, Clone)]
pub struct TaskStatusUpdate {
    pub name: String,
    pub status: String,
    pub result: Option<String>,
    pub duration_secs: Option<f64>,
    pub success: Option<bool>,
    pub error: Option<String>,
}

/// Log output (stdout/stderr)
#[derive(Debug, Clone)]
pub struct LogOutputUpdate {
    pub stream: String,
    pub message: String,
}

/// Helper function to parse u64 from string fields
fn parse_u64(fields: &std::collections::HashMap<String, String>, key: &str) -> Option<u64> {
    fields.get(key)?.parse().ok()
}

/// Helper function to parse f64 from string fields
fn parse_f64(fields: &std::collections::HashMap<String, String>, key: &str) -> Option<f64> {
    fields.get(key)?.parse().ok()
}

/// Helper function to parse bool from string fields
fn parse_bool(fields: &std::collections::HashMap<String, String>, key: &str) -> Option<bool> {
    let value = fields.get(key)?;
    match value.trim_matches('"').to_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

impl NixProgressUpdate {
    pub fn from_fields(fields: &std::collections::HashMap<String, String>) -> Option<Self> {
        Some(Self {
            activity_id: parse_u64(fields, "activity_id")?,
            done: parse_u64(fields, "done")?,
            expected: parse_u64(fields, "expected")?,
            running: parse_u64(fields, "running").unwrap_or(0),
            failed: parse_u64(fields, "failed").unwrap_or(0),
        })
    }
}

impl BuildPhaseUpdate {
    pub fn from_fields(fields: &std::collections::HashMap<String, String>) -> Option<Self> {
        Some(Self {
            activity_id: parse_u64(fields, "activity_id")?,
            phase: fields.get("phase")?.clone(),
        })
    }
}

impl BuildLogUpdate {
    pub fn from_fields(fields: &std::collections::HashMap<String, String>) -> Option<Self> {
        Some(Self {
            activity_id: parse_u64(fields, "activity_id")?,
            line: fields.get("line")?.clone(),
        })
    }
}

impl DownloadProgressUpdate {
    pub fn from_fields(fields: &std::collections::HashMap<String, String>) -> Option<Self> {
        Some(Self {
            activity_id: parse_u64(fields, "activity_id")?,
            bytes_downloaded: parse_u64(fields, "bytes_downloaded")?,
            total_bytes: parse_u64(fields, "total_bytes"),
        })
    }
}

impl EvaluationProgressUpdate {
    pub fn from_fields(fields: &std::collections::HashMap<String, String>) -> Option<Self> {
        let activity_id = parse_u64(fields, "activity_id")?;
        let total_files_evaluated = parse_u64(fields, "total_files_evaluated").unwrap_or(0);

        let files_str = fields.get("files")?;
        let latest_files: Vec<String> = files_str
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(", ")
            .map(|s| s.trim_matches('"').to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Some(Self {
            activity_id,
            total_files_evaluated,
            latest_files,
        })
    }
}

impl TaskStatusUpdate {
    pub fn from_fields(fields: &std::collections::HashMap<String, String>) -> Option<Self> {
        let name = fields.get("devenv.ui.task.name")
            .or_else(|| fields.get("name"))?
            .trim_matches('"')
            .to_string();

        let status = fields.get("devenv.ui.status")
            .or_else(|| fields.get("status"))
            .map(|s| s.trim_matches('"').to_string());

        let result = fields.get("devenv.ui.status.result")
            .or_else(|| fields.get("result"))
            .map(|s| s.trim_matches('"').to_string());

        let duration_secs = parse_f64(fields, "duration_secs");
        let success = parse_bool(fields, "success");
        let error = fields.get("error").map(|s| s.trim_matches('"').to_string());

        Some(Self {
            name,
            status: status.unwrap_or_else(|| "unknown".to_string()),
            result,
            duration_secs,
            success,
            error,
        })
    }
}

impl LogOutputUpdate {
    pub fn from_fields(fields: &std::collections::HashMap<String, String>) -> Option<Self> {
        Some(Self {
            stream: fields.get("devenv.ui.log.stream")?.clone(),
            message: fields.get("devenv.ui.log.message")?.clone(),
        })
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
