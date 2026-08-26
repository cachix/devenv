use serde::Serialize;
use std::collections::BTreeMap;
use tokio::time::{Duration, Instant};

pub(crate) use devenv_core::VerbosityLevel;

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

/// Condition that satisfies a dependency edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DependencyKind {
    /// Satisfied once execution begins; later exit status is ignored.
    Started,
    /// Satisfied when a process reaches Ready; the default for process edges.
    #[default]
    Ready,
    /// Satisfied when a one-shot succeeds; the default for one-shot edges.
    Succeeded,
    /// Satisfied by any terminal outcome without propagating failure.
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

/// Navigate to `value["devenv"][field]`.
fn get_devenv_field<'a>(
    value: &'a serde_json::Value,
    field: &str,
) -> Option<&'a serde_json::Value> {
    value.get("devenv").and_then(|d| d.get(field))
}

/// Read the `devenv.env` object from a task output JSON value.
pub fn get_devenv_env(
    value: &serde_json::Value,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    get_devenv_field(value, "env").and_then(|e| e.as_object())
}

/// Iterate over the `devenv.messages` strings in a task output JSON value.
fn iter_devenv_messages(value: &serde_json::Value) -> impl Iterator<Item = &str> {
    get_devenv_field(value, "messages")
        .and_then(|m| m.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
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

    /// Extract all `devenv.messages` strings from task outputs.
    ///
    /// Each task's JSON output may contain `{"devenv": {"messages": ["msg1", "msg2"]}}`.
    /// Messages are collected in task name order (BTreeMap iteration), preserving
    /// array order within each task.
    pub fn collect_messages(&self) -> Vec<String> {
        self.0
            .values()
            .flat_map(iter_devenv_messages)
            .map(String::from)
            .collect()
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

/// Evaluate a process dependency from coherent lifecycle facts. Presentation
/// phases intentionally cannot represent enough information for this table.
pub fn is_process_status_dep_satisfied(
    status: devenv_processes::ProcessStatus,
    kind: &DependencyKind,
) -> DepSatisfaction {
    use devenv_processes::{
        ChildState, ExitOutcome, ReadinessState, RestartDecision, StateTransition, StopReason,
        TargetState,
    };

    if status.transition == Some(StateTransition::WaitingForDependencies) {
        return DepSatisfaction::NotYet;
    }

    if status.transition == Some(StateTransition::Terminating) {
        return match kind {
            DependencyKind::Started if status.child.was_spawned() => DepSatisfaction::Satisfied,
            DependencyKind::Started => DepSatisfaction::NotYet,
            DependencyKind::Completed => DepSatisfaction::NotYet,
            DependencyKind::Ready | DependencyKind::Succeeded => DepSatisfaction::NeverSatisfiable,
        };
    }

    if matches!(
        status.transition,
        Some(StateTransition::Launching | StateTransition::Replacing)
    ) || status.restart == RestartDecision::Pending
    {
        return match kind {
            DependencyKind::Started if status.child.was_spawned() => DepSatisfaction::Satisfied,
            _ => DepSatisfaction::NotYet,
        };
    }

    match status.child {
        ChildState::Running => match kind {
            DependencyKind::Started => DepSatisfaction::Satisfied,
            DependencyKind::Ready
                if matches!(
                    status.readiness,
                    ReadinessState::Ready | ReadinessState::NotRequired
                ) =>
            {
                DepSatisfaction::Satisfied
            }
            _ => DepSatisfaction::NotYet,
        },
        ChildState::Exited(exit) => match kind {
            DependencyKind::Started | DependencyKind::Completed => DepSatisfaction::Satisfied,
            DependencyKind::Succeeded
                if exit == ExitOutcome::Success && status.restart != RestartDecision::Exhausted =>
            {
                DepSatisfaction::Satisfied
            }
            DependencyKind::Ready | DependencyKind::Succeeded => DepSatisfaction::NeverSatisfiable,
        },
        ChildState::Terminated => match kind {
            DependencyKind::Started | DependencyKind::Completed => DepSatisfaction::Satisfied,
            DependencyKind::Ready => DepSatisfaction::NotYet,
            DependencyKind::Succeeded => DepSatisfaction::NeverSatisfiable,
        },
        ChildState::NeverSpawned => {
            let graph_failed = matches!(
                status.target,
                TargetState::Stopped(StopReason::DependencyFailure | StopReason::LaunchFailure)
            );
            match kind {
                DependencyKind::Completed => DepSatisfaction::Satisfied,
                DependencyKind::Started | DependencyKind::Ready if !graph_failed => {
                    DepSatisfaction::NotYet
                }
                DependencyKind::Started | DependencyKind::Ready | DependencyKind::Succeeded => {
                    DepSatisfaction::NeverSatisfiable
                }
            }
        }
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
///
/// Process dependencies are evaluated from `ProcessStatus`; this function
/// covers graph-owned task execution only.
pub fn is_dep_satisfied(status: &TaskExecutionState, kind: &DependencyKind) -> DepSatisfaction {
    match status {
        TaskExecutionState::NotScheduled | TaskExecutionState::WaitingForDependencies => {
            DepSatisfaction::NotYet
        }

        TaskExecutionState::Running { .. } => match kind {
            DependencyKind::Started => DepSatisfaction::Satisfied,
            _ => DepSatisfaction::NotYet,
        },

        TaskExecutionState::Finished(completed) => is_completed_dep_satisfied(completed, kind),
    }
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
pub enum TaskExecutionState {
    NotScheduled,
    WaitingForDependencies,
    Running { since: Instant },
    Finished(TaskCompleted),
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ──────────────────────────────────────────────────────

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

    #[test]
    fn process_status_dependency_matrix() {
        use DepSatisfaction::{NeverSatisfiable as Never, NotYet, Satisfied as Yes};
        use devenv_processes::*;

        let mut launching = ProcessStatus::waiting();
        launching.transition = Some(StateTransition::Launching);

        let mut scheduled = ProcessStatus::stopped(
            StopReason::LaunchFailure,
            ChildState::Exited(ExitOutcome::Failure),
        );
        scheduled.target = TargetState::Running;
        scheduled.restart = RestartDecision::Pending;

        let mut exhausted = ProcessStatus::stopped(
            StopReason::LaunchFailure,
            ChildState::Exited(ExitOutcome::Failure),
        );
        exhausted.restart = RestartDecision::Exhausted;

        let rows = [
            (ProcessStatus::waiting(), [NotYet, NotYet, NotYet, NotYet]),
            (launching, [NotYet, NotYet, NotYet, NotYet]),
            (
                ProcessStatus::running(true, StateTransition::Launching),
                [Yes, NotYet, NotYet, NotYet],
            ),
            (
                ProcessStatus::running(false, StateTransition::Launching),
                [Yes, Yes, NotYet, NotYet],
            ),
            (scheduled, [Yes, NotYet, NotYet, NotYet]),
            (
                ProcessStatus::stopped(
                    StopReason::ManagerShutdown,
                    ChildState::Exited(ExitOutcome::Success),
                ),
                [Yes, Never, Yes, Yes],
            ),
            (
                ProcessStatus::stopped(
                    StopReason::ManagerShutdown,
                    ChildState::Exited(ExitOutcome::Failure),
                ),
                [Yes, Never, Never, Yes],
            ),
            (exhausted, [Yes, Never, Never, Yes]),
            (
                ProcessStatus::stopped(StopReason::User, ChildState::Terminated),
                [Yes, NotYet, Never, Yes],
            ),
            (ProcessStatus::not_started(), [NotYet, NotYet, Never, Yes]),
            (
                ProcessStatus::stopped(StopReason::DependencyFailure, ChildState::NeverSpawned),
                [Never, Never, Never, Yes],
            ),
        ];

        for (status, expected) in rows {
            assert!(status.is_valid(), "invalid test status: {status:?}");
            for (kind, expected) in ALL_KINDS.iter().zip(expected) {
                assert_eq!(
                    is_process_status_dep_satisfied(status, kind),
                    expected,
                    "status={status:?}, kind={kind:?}"
                );
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
            let actual = is_dep_satisfied(&TaskExecutionState::WaitingForDependencies, kind);
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
        let status = TaskExecutionState::Running {
            since: Instant::now(),
        };

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
    fn process_dep_satisfied_ready_spot_check() {
        let status = devenv_processes::ProcessStatus::running(
            false,
            devenv_processes::StateTransition::Launching,
        );
        assert_eq!(
            is_process_status_dep_satisfied(status, &DependencyKind::Ready),
            DepSatisfaction::Satisfied,
        );
        assert_eq!(
            is_process_status_dep_satisfied(status, &DependencyKind::Succeeded),
            DepSatisfaction::NotYet,
        );
    }

    #[test]
    fn dep_satisfied_completed_delegates() {
        // Spot check: delegates to is_completed_dep_satisfied
        let status = TaskExecutionState::Finished(make_failed());

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
                let status = TaskExecutionState::Finished(completed.clone());
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
