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

/// Dependency kind: controls when a dependency is considered satisfied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DependencyKind {
    /// Wait for task to begin execution (hard dependency).
    /// Satisfied once the task is running or completed.
    Started,
    /// Wait for task to be ready/healthy (default, hard dependency).
    /// - For oneshot tasks: wait for successful completion
    /// - For process tasks: wait for Ready state
    ///
    /// Propagates failure: if the dependency fails, this task fails too.
    #[default]
    Ready,
    /// Wait for task to exit successfully (hard dependency).
    /// Satisfied only when the task completes with exit code 0 (or is skipped).
    Succeeded,
    /// Wait for task to complete/shutdown (soft dependency).
    /// Satisfied when the task finishes, regardless of exit code.
    /// Does NOT propagate failure: if the dependency fails, this task still runs.
    Completed,
}

/// Dependency specification with optional suffix
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencySpec {
    /// Task name without suffix
    pub name: String,
    /// Dependency kind, or None for default behavior.
    /// Default: Ready for process tasks, Succeeded for oneshot tasks
    pub kind: Option<DependencyKind>,
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TasksStatus {
    pub pending: usize,
    pub running: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub dependency_failed: usize,
    pub cancelled: usize,
    /// Tasks that failed but are exclusively `@completed` (soft) dependencies
    pub soft_failed: usize,
    /// Tasks marked DependencyFailed whose root cause is exclusively a soft failure
    pub soft_dependency_failed: usize,
}

impl TasksStatus {
    /// Create a new empty TasksStatus
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if all tasks are complete (no pending or running tasks)
    pub fn is_complete(&self) -> bool {
        self.pending == 0 && self.running == 0
    }

    /// Check if any tasks failed (excluding soft `@completed`-only failures)
    pub fn has_failures(&self) -> bool {
        self.failed > self.soft_failed || self.dependency_failed > self.soft_dependency_failed
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

/// Read the `devenv.env` object from a task output JSON value.
pub fn get_devenv_env(
    value: &serde_json::Value,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    value
        .get("devenv")
        .and_then(|d| d.get("env"))
        .and_then(|e| e.as_object())
}

/// Get or create the mutable `devenv.env` object in a task output JSON value.
pub(crate) fn get_or_create_devenv_env_mut(
    value: &mut serde_json::Value,
) -> Option<&mut serde_json::Map<String, serde_json::Value>> {
    value
        .as_object_mut()
        .and_then(|obj| {
            obj.entry("devenv")
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
        })
        .and_then(|devenv| {
            devenv
                .entry("env")
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
        })
}

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

#[derive(Clone, Debug, Serialize)]
pub struct Outputs(pub BTreeMap<String, serde_json::Value>);

#[derive(Debug, Clone)]
pub struct Output(pub Option<serde_json::Value>);

impl Outputs {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Extract all `devenv.env` vars from task outputs into a flat map.
    ///
    /// Each task's JSON output may contain `{"devenv": {"env": {"KEY": "VALUE"}}}`.
    /// This merges them all into a single `BTreeMap<String, String>`.
    pub fn collect_env_exports(&self) -> BTreeMap<String, String> {
        let mut envs = BTreeMap::new();
        for value in self.0.values() {
            if let Some(env_obj) = get_devenv_env(value) {
                for (env_key, env_value) in env_obj {
                    if let Some(env_str) = env_value.as_str() {
                        envs.insert(env_key.clone(), env_str.to_string());
                    }
                }
            }
        }
        envs
    }
}

impl Default for Outputs {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for Outputs {
    type Target = BTreeMap<String, serde_json::Value>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Outputs {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
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

/// Result of checking whether a dependency is satisfied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepSatisfaction {
    /// The dependency is satisfied; the dependent can proceed.
    Satisfied,
    /// The dependency is not yet in a satisfying state; keep waiting.
    NotYet,
    /// The dependency completed in a way that can never satisfy the
    /// required kind (e.g. a failed task for `@ready`). Treat as failure.
    NeverSatisfiable,
}

/// Check whether a process task status satisfies the given dependency kind.
pub fn is_process_dep_satisfied(
    status: &ProcessTaskStatus,
    kind: &DependencyKind,
) -> DepSatisfaction {
    match (status.phase, kind) {
        // Waiting: nothing satisfied yet
        (ProcessPhase::Waiting, _) => DepSatisfaction::NotYet,

        // NotStarted (auto start off): @completed is satisfied immediately,
        // @started/@ready keep waiting (the process can be started manually later),
        // @succeeded is never satisfiable without actual execution.
        (ProcessPhase::NotStarted, DependencyKind::Completed) => DepSatisfaction::Satisfied,
        (ProcessPhase::NotStarted, DependencyKind::Started | DependencyKind::Ready) => {
            DepSatisfaction::NotYet
        }
        (ProcessPhase::NotStarted, _) => DepSatisfaction::NeverSatisfiable,

        // Starting: @started is satisfied, everything else not yet
        (ProcessPhase::Starting, DependencyKind::Started) => DepSatisfaction::Satisfied,
        (ProcessPhase::Starting, _) => DepSatisfaction::NotYet,

        // Ready: @started and @ready are satisfied
        (ProcessPhase::Ready, DependencyKind::Started | DependencyKind::Ready) => {
            DepSatisfaction::Satisfied
        }
        (ProcessPhase::Ready, _) => DepSatisfaction::NotYet,

        // GaveUp: @completed is satisfied, others are never satisfiable
        (ProcessPhase::GaveUp, DependencyKind::Completed) => DepSatisfaction::Satisfied,
        (ProcessPhase::GaveUp, _) => DepSatisfaction::NeverSatisfiable,
    }
}

/// Check whether a completed task status satisfies the given dependency kind.
fn is_completed_dep_satisfied(completed: &TaskCompleted, kind: &DependencyKind) -> DepSatisfaction {
    match (completed, kind) {
        // @started — satisfied by any completion
        (_, DependencyKind::Started) => DepSatisfaction::Satisfied,

        // @ready — success or skipped
        (TaskCompleted::Success(_, _), DependencyKind::Ready) => DepSatisfaction::Satisfied,
        (TaskCompleted::Skipped(_), DependencyKind::Ready) => DepSatisfaction::Satisfied,

        // @succeeded — exited with code 0 or skipped
        (TaskCompleted::Success(_, _), DependencyKind::Succeeded) => DepSatisfaction::Satisfied,
        (TaskCompleted::Skipped(_), DependencyKind::Succeeded) => DepSatisfaction::Satisfied,

        // @completed — any completion (soft)
        (_, DependencyKind::Completed) => DepSatisfaction::Satisfied,

        // Completed but doesn't satisfy the required kind
        (_, _) => DepSatisfaction::NeverSatisfiable,
    }
}

/// Check whether `status` satisfies the given `kind`.
pub fn is_dep_satisfied(status: &TaskStatus, kind: &DependencyKind) -> DepSatisfaction {
    match status {
        TaskStatus::Pending => DepSatisfaction::NotYet,

        TaskStatus::Oneshot(OneshotStatus::Running(_)) => match kind {
            DependencyKind::Started => DepSatisfaction::Satisfied,
            _ => DepSatisfaction::NotYet,
        },

        TaskStatus::Process(ps) => is_process_dep_satisfied(ps, kind),

        TaskStatus::Completed(completed) => is_completed_dep_satisfied(completed, kind),
    }
}

#[derive(Debug, Clone)]
pub enum OneshotStatus {
    Running(Instant),
}

/// Prefix used for process task names (e.g. "devenv:processes:http-server").
pub const PROCESS_TASK_PREFIX: &str = "devenv:processes:";

/// Strip the `devenv:processes:` prefix to get the short process name.
pub fn process_name(task_name: &str) -> &str {
    task_name
        .strip_prefix(PROCESS_TASK_PREFIX)
        .unwrap_or(task_name)
}

pub use devenv_processes::ProcessPhase;

#[derive(Debug, Clone)]
pub struct ProcessTaskStatus {
    pub name: String,
    pub phase: ProcessPhase,
}

#[derive(Debug, Clone)]
pub enum TaskStatus {
    Pending,
    Oneshot(OneshotStatus),
    Process(ProcessTaskStatus),
    Completed(TaskCompleted),
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ──────────────────────────────────────────────────────

    fn make_process_status(phase: ProcessPhase) -> ProcessTaskStatus {
        ProcessTaskStatus {
            name: "test".to_string(),
            phase,
        }
    }

    fn make_success() -> TaskCompleted {
        TaskCompleted::Success(Duration::from_secs(0), Output(None))
    }

    fn make_skipped_cached() -> TaskCompleted {
        TaskCompleted::Skipped(Skipped::Cached(Output(None)))
    }

    fn make_skipped_no_command() -> TaskCompleted {
        TaskCompleted::Skipped(Skipped::NoCommand)
    }

    fn make_failed() -> TaskCompleted {
        TaskCompleted::Failed(
            Duration::from_secs(0),
            TaskFailure {
                stdout: vec![],
                stderr: vec![],
                error: "boom".to_string(),
            },
        )
    }

    fn make_dependency_failed() -> TaskCompleted {
        TaskCompleted::DependencyFailed
    }

    fn make_cancelled_running() -> TaskCompleted {
        TaskCompleted::Cancelled(Some(Duration::from_secs(0)))
    }

    fn make_cancelled_not_running() -> TaskCompleted {
        TaskCompleted::Cancelled(None)
    }

    const ALL_KINDS: [DependencyKind; 4] = [
        DependencyKind::Started,
        DependencyKind::Ready,
        DependencyKind::Succeeded,
        DependencyKind::Completed,
    ];

    const ALL_PHASES: [ProcessPhase; 5] = [
        ProcessPhase::NotStarted,
        ProcessPhase::Waiting,
        ProcessPhase::Starting,
        ProcessPhase::Ready,
        ProcessPhase::GaveUp,
    ];

    // ── process_name ────────────────────────────────────────────────

    #[test]
    fn process_name_strips_prefix() {
        assert_eq!(process_name("devenv:processes:http-server"), "http-server");
    }

    #[test]
    fn process_name_strips_prefix_empty_suffix() {
        assert_eq!(process_name("devenv:processes:"), "");
    }

    #[test]
    fn process_name_returns_input_without_prefix() {
        assert_eq!(process_name("some-task"), "some-task");
    }

    #[test]
    fn process_name_returns_empty_for_empty_input() {
        assert_eq!(process_name(""), "");
    }

    #[test]
    fn process_name_partial_prefix_unchanged() {
        assert_eq!(process_name("devenv:processes"), "devenv:processes");
    }

    // ── is_process_dep_satisfied ────────────────────────────────────

    /// Table driven test covering every (ProcessPhase, DependencyKind) pair.
    #[test]
    fn process_dep_satisfied_exhaustive() {
        use DepSatisfaction::*;

        let started = DependencyKind::Started;
        let ready = DependencyKind::Ready;
        let succeeded = DependencyKind::Succeeded;
        let completed = DependencyKind::Completed;

        // (phase, kind) -> expected
        let table: Vec<(ProcessPhase, DependencyKind, DepSatisfaction)> = vec![
            // Waiting: always NotYet
            (ProcessPhase::Waiting, started, NotYet),
            (ProcessPhase::Waiting, ready, NotYet),
            (ProcessPhase::Waiting, succeeded, NotYet),
            (ProcessPhase::Waiting, completed, NotYet),
            // NotStarted
            (ProcessPhase::NotStarted, started, NotYet),
            (ProcessPhase::NotStarted, ready, NotYet),
            (ProcessPhase::NotStarted, succeeded, NeverSatisfiable),
            (ProcessPhase::NotStarted, completed, Satisfied),
            // Starting
            (ProcessPhase::Starting, started, Satisfied),
            (ProcessPhase::Starting, ready, NotYet),
            (ProcessPhase::Starting, succeeded, NotYet),
            (ProcessPhase::Starting, completed, NotYet),
            // Ready
            (ProcessPhase::Ready, started, Satisfied),
            (ProcessPhase::Ready, ready, Satisfied),
            (ProcessPhase::Ready, succeeded, NotYet),
            (ProcessPhase::Ready, completed, NotYet),
            // GaveUp
            (ProcessPhase::GaveUp, started, NeverSatisfiable),
            (ProcessPhase::GaveUp, ready, NeverSatisfiable),
            (ProcessPhase::GaveUp, succeeded, NeverSatisfiable),
            (ProcessPhase::GaveUp, completed, Satisfied),
        ];

        for (phase, kind, expected) in &table {
            let status = make_process_status(*phase);
            let actual = is_process_dep_satisfied(&status, kind);
            assert_eq!(
                actual, *expected,
                "phase={:?}, kind={:?}: expected {:?}, got {:?}",
                phase, kind, expected, actual
            );
        }
    }

    /// Verify the table covers every combination.
    #[test]
    fn process_dep_satisfied_all_combinations_covered() {
        for phase in &ALL_PHASES {
            for kind in &ALL_KINDS {
                let status = make_process_status(*phase);
                // Should not panic for any combination
                let _ = is_process_dep_satisfied(&status, kind);
            }
        }
    }

    // ── is_completed_dep_satisfied ──────────────────────────────────

    #[test]
    fn completed_dep_satisfied_exhaustive() {
        use DepSatisfaction::*;
        use DependencyKind::*;

        // (completed_variant, kind) -> expected
        let table: Vec<(TaskCompleted, DependencyKind, DepSatisfaction)> = vec![
            // Success
            (make_success(), Started, Satisfied),
            (make_success(), Ready, Satisfied),
            (make_success(), Succeeded, Satisfied),
            (make_success(), Completed, Satisfied),
            // Skipped (Cached)
            (make_skipped_cached(), Started, Satisfied),
            (make_skipped_cached(), Ready, Satisfied),
            (make_skipped_cached(), Succeeded, Satisfied),
            (make_skipped_cached(), Completed, Satisfied),
            // Skipped (NoCommand)
            (make_skipped_no_command(), Started, Satisfied),
            (make_skipped_no_command(), Ready, Satisfied),
            (make_skipped_no_command(), Succeeded, Satisfied),
            (make_skipped_no_command(), Completed, Satisfied),
            // Failed
            (make_failed(), Started, Satisfied),
            (make_failed(), Ready, NeverSatisfiable),
            (make_failed(), Succeeded, NeverSatisfiable),
            (make_failed(), Completed, Satisfied),
            // DependencyFailed
            (make_dependency_failed(), Started, Satisfied),
            (make_dependency_failed(), Ready, NeverSatisfiable),
            (make_dependency_failed(), Succeeded, NeverSatisfiable),
            (make_dependency_failed(), Completed, Satisfied),
            // Cancelled (was running)
            (make_cancelled_running(), Started, Satisfied),
            (make_cancelled_running(), Ready, NeverSatisfiable),
            (make_cancelled_running(), Succeeded, NeverSatisfiable),
            (make_cancelled_running(), Completed, Satisfied),
            // Cancelled (was not running)
            (make_cancelled_not_running(), Started, Satisfied),
            (make_cancelled_not_running(), Ready, NeverSatisfiable),
            (make_cancelled_not_running(), Succeeded, NeverSatisfiable),
            (make_cancelled_not_running(), Completed, Satisfied),
        ];

        for (completed, kind, expected) in &table {
            let actual = is_completed_dep_satisfied(completed, kind);
            assert_eq!(
                actual, *expected,
                "completed={:?}, kind={:?}: expected {:?}, got {:?}",
                completed, kind, expected, actual
            );
        }
    }

    // ── is_dep_satisfied ────────────────────────────────────────────

    #[test]
    fn dep_satisfied_pending_always_not_yet() {
        for kind in &ALL_KINDS {
            let actual = is_dep_satisfied(&TaskStatus::Pending, kind);
            assert_eq!(
                actual,
                DepSatisfaction::NotYet,
                "Pending with kind={:?} should be NotYet",
                kind
            );
        }
    }

    #[test]
    fn dep_satisfied_oneshot_running() {
        let status = TaskStatus::Oneshot(OneshotStatus::Running(Instant::now()));

        assert_eq!(
            is_dep_satisfied(&status, &DependencyKind::Started),
            DepSatisfaction::Satisfied,
        );
        assert_eq!(
            is_dep_satisfied(&status, &DependencyKind::Ready),
            DepSatisfaction::NotYet,
        );
        assert_eq!(
            is_dep_satisfied(&status, &DependencyKind::Succeeded),
            DepSatisfaction::NotYet,
        );
        assert_eq!(
            is_dep_satisfied(&status, &DependencyKind::Completed),
            DepSatisfaction::NotYet,
        );
    }

    #[test]
    fn dep_satisfied_process_delegates() {
        // Spot check: delegates to is_process_dep_satisfied
        let ps = make_process_status(ProcessPhase::Ready);
        let status = TaskStatus::Process(ps);

        assert_eq!(
            is_dep_satisfied(&status, &DependencyKind::Ready),
            DepSatisfaction::Satisfied,
        );
        assert_eq!(
            is_dep_satisfied(&status, &DependencyKind::Succeeded),
            DepSatisfaction::NotYet,
        );
    }

    #[test]
    fn dep_satisfied_process_delegates_all_combinations() {
        for phase in &ALL_PHASES {
            for kind in &ALL_KINDS {
                let ps = make_process_status(*phase);
                let expected = is_process_dep_satisfied(&ps, kind);
                let status = TaskStatus::Process(make_process_status(*phase));
                let actual = is_dep_satisfied(&status, kind);
                assert_eq!(
                    actual, expected,
                    "Process delegation mismatch: phase={:?}, kind={:?}",
                    phase, kind
                );
            }
        }
    }

    #[test]
    fn dep_satisfied_completed_delegates() {
        // Spot check: delegates to is_completed_dep_satisfied
        let status = TaskStatus::Completed(make_failed());

        assert_eq!(
            is_dep_satisfied(&status, &DependencyKind::Started),
            DepSatisfaction::Satisfied,
        );
        assert_eq!(
            is_dep_satisfied(&status, &DependencyKind::Ready),
            DepSatisfaction::NeverSatisfiable,
        );
    }

    #[test]
    fn dep_satisfied_completed_delegates_all_variants() {
        let completed_variants: Vec<TaskCompleted> = vec![
            make_success(),
            make_skipped_cached(),
            make_skipped_no_command(),
            make_failed(),
            make_dependency_failed(),
            make_cancelled_running(),
            make_cancelled_not_running(),
        ];

        for completed in &completed_variants {
            for kind in &ALL_KINDS {
                let expected = is_completed_dep_satisfied(completed, kind);
                let status = TaskStatus::Completed(completed.clone());
                let actual = is_dep_satisfied(&status, kind);
                assert_eq!(
                    actual, expected,
                    "Completed delegation mismatch: completed={:?}, kind={:?}",
                    completed, kind
                );
            }
        }
    }
}
