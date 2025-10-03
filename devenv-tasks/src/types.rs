use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Display;
use tokio::time::{Duration, Instant};

/// Verbosity level for task output
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerbosityLevel {
    /// Silence all output
    Quiet,
    /// Normal output level
    Normal,
    /// Verbose output with additional details
    Verbose,
}

impl Display for VerbosityLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerbosityLevel::Quiet => write!(f, "quiet"),
            VerbosityLevel::Normal => write!(f, "normal"),
            VerbosityLevel::Verbose => write!(f, "verbose"),
        }
    }
}

#[derive(Serialize)]
pub struct Outputs(pub BTreeMap<String, serde_json::Value>);

#[derive(Debug, Clone)]
pub struct Output(pub Option<serde_json::Value>);

impl std::ops::Deref for Outputs {
    type Target = BTreeMap<String, serde_json::Value>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub type LinesOutput = Vec<(std::time::Instant, String)>;

#[derive(Debug, Clone)]
pub struct TaskFailure {
    pub stdout: LinesOutput,
    pub stderr: LinesOutput,
    pub error: String,
}

#[derive(Debug, Clone)]
pub enum Skipped {
    Cached(Output),
    NoCommand,
}

#[derive(Debug, Clone)]
pub enum TaskCompleted {
    Success(Duration, Output),
    Skipped(Skipped),
    Failed(Duration, TaskFailure),
    DependencyFailed,
    Cancelled(Duration),
}

impl TaskCompleted {
    pub fn has_failed(&self) -> bool {
        matches!(
            self,
            TaskCompleted::Failed(_, _) | TaskCompleted::DependencyFailed
        )
    }
}

#[derive(Debug, Clone)]
pub enum TaskStatus {
    Pending,
    Running(Instant),
    Completed(TaskCompleted),
}

pub struct TasksStatus {
    pub pending: usize,
    pub running: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub dependency_failed: usize,
    pub cancelled: usize,
}

impl Default for TasksStatus {
    fn default() -> Self {
        Self::new()
    }
}

impl TasksStatus {
    /// Create a new empty TasksStatus
    pub fn new() -> Self {
        Self {
            pending: 0,
            running: 0,
            succeeded: 0,
            failed: 0,
            skipped: 0,
            dependency_failed: 0,
            cancelled: 0,
        }
    }
}
