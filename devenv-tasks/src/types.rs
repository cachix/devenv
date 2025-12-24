use serde::Serialize;
use std::collections::BTreeMap;
use tokio::time::{Duration, Instant};

/// Task type: oneshot (run once) or process (long-running)
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum TaskType {
    /// Task runs once and completes (default)
    #[default]
    Oneshot,
    /// Task is a long-running process
    Process,
}

/// Dependency kind: wait for ready state or completion
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DependencyKind {
    /// Wait for task to be ready/healthy (default)
    /// - For oneshot tasks: wait for successful completion
    /// - For process tasks: wait for ProcessReady state
    #[default]
    Ready,
    /// Wait for task to complete/shutdown
    /// - For oneshot tasks: same as Ready (wait for completion)
    /// - For process tasks: wait for process to shut down
    Complete,
}

/// Dependency specification with optional suffix
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencySpec {
    /// Task name without suffix
    pub name: String,
    /// Dependency kind (Ready or Complete)
    pub kind: DependencyKind,
}

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
}

#[derive(Debug, Clone)]
pub enum TaskStatus {
    Pending,
    Running(Instant),
    /// Process task is ready and healthy (not used yet, for future process support)
    ProcessReady,
    Completed(TaskCompleted),
}
