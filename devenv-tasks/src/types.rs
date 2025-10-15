use serde::Serialize;
use std::collections::BTreeMap;
use tokio::time::{Duration, Instant};

/// Verbosity levels for task execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VerbosityLevel {
    /// Minimal output, only errors
    Quiet,
    /// Standard output level
    Normal,
    /// Detailed output including debug information
    Verbose,
}

impl std::fmt::Display for VerbosityLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerbosityLevel::Quiet => write!(f, "quiet"),
            VerbosityLevel::Normal => write!(f, "normal"),
            VerbosityLevel::Verbose => write!(f, "verbose"),
        }
    }
}

/// Current status counters for all tasks in execution
#[derive(Debug, Clone, PartialEq, Eq)]
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

    /// Check if all tasks are complete (no pending or running tasks)
    pub fn is_complete(&self) -> bool {
        self.pending == 0 && self.running == 0
    }

    /// Check if any tasks failed
    pub fn has_failures(&self) -> bool {
        self.failed > 0 || self.dependency_failed > 0
    }

    /// Get total number of tasks
    pub fn total(&self) -> usize {
        self.pending
            + self.running
            + self.succeeded
            + self.failed
            + self.skipped
            + self.dependency_failed
            + self.cancelled
    }

    /// Get total number of completed tasks
    pub fn completed(&self) -> usize {
        self.succeeded + self.failed + self.skipped + self.dependency_failed + self.cancelled
    }
}

/// Configuration for a single task
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskConfig {
    pub name: String,
    pub after: Vec<String>,
    pub before: Vec<String>,
    pub command: Option<String>,
    pub status: Option<String>,
    pub exec_if_modified: Vec<String>,
    pub inputs: Option<serde_json::Value>,
}

/// Execution mode determining which tasks to run
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, clap::ValueEnum,
)]
pub enum RunMode {
    /// Run only the specified task without dependencies
    Single,
    /// Run the specified task and all tasks that depend on it (downstream tasks)
    After,
    /// Run all dependency tasks first, then the specified task (upstream tasks)
    Before,
    /// Run the complete dependency graph (upstream and downstream tasks)
    All,
}

/// Configuration for a complete task execution run
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub tasks: Vec<TaskConfig>,
    pub roots: Vec<String>,
    pub run_mode: RunMode,
}

/// Output data from tasks
pub type TaskOutputs = serde_json::Value;

/// Terminal detection utility
pub fn is_tty() -> bool {
    console::Term::stdout().is_term() && console::Term::stderr().is_term()
}

/// UI modes available for task execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    /// Full interactive TUI with enhanced features
    Tui,
    /// Simple terminal output with progress bars
    Terminal,
    /// No output, only tracing events
    Headless,
}

/// Determine the appropriate UI mode based on verbosity and TTY availability
pub fn determine_ui_mode(verbosity: VerbosityLevel, has_tui_sender: bool) -> UiMode {
    if has_tui_sender {
        // TUI is active, use headless mode to avoid terminal conflicts
        UiMode::Headless
    } else if verbosity == VerbosityLevel::Quiet {
        UiMode::Headless
    } else if is_tty() {
        // We have a TTY, use terminal mode
        UiMode::Terminal
    } else {
        // No TTY (redirected output, CI, etc.), use headless mode
        UiMode::Headless
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
    /// Cancelled externally.
    /// If the job was running, contains the duration it ran for.
    Cancelled(Option<Duration>),
}

impl TaskCompleted {
    pub fn has_failed(&self) -> bool {
        matches!(
            self,
            TaskCompleted::Failed(_, _) | TaskCompleted::DependencyFailed
        )
    }

    // TODO: use this everywhere instead of ad-hoc strings
    pub fn to_tracing_status(&self) -> &'static str {
        match self {
            TaskCompleted::Success(_, _) => "success",
            TaskCompleted::Skipped(_) => "skipped",
            TaskCompleted::Failed(_, _) => "failed",
            TaskCompleted::DependencyFailed => "dependency_failed",
            TaskCompleted::Cancelled(_) => "cancelled",
        }
    }
}

#[derive(Debug, Clone)]
pub enum TaskStatus {
    Pending,
    Running(Instant),
    Completed(TaskCompleted),
}
