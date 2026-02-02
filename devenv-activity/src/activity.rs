//! Activity guard that tracks an activity's lifecycle.

use std::cell::RefCell;
use std::ops::Deref;

use tracing::Span;

use crate::Timestamp;
use crate::builders::{
    BuildBuilder, CommandBuilder, EvaluateBuilder, FetchBuilder, OperationBuilder, TaskBuilder,
};
use crate::events::{
    ActivityEvent, ActivityLevel, ActivityOutcome, Build, Command, Evaluate, Fetch, FetchKind,
    Operation, Task,
};
use crate::stack::{ACTIVITY_STACK, get_current_stack, send_activity_event};

/// Activity type for tracking which kind of activity this is
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityType {
    Build,
    Fetch(FetchKind),
    Evaluate,
    Task,
    Command,
    Operation,
}

/// Guard that tracks an activity's lifecycle via tracing spans.
/// Activity is Send + Sync, allowing storage in Mutex for async callbacks.
#[must_use = "Activity will complete immediately if dropped"]
pub struct Activity {
    span: Span,
    id: u64,
    activity_type: ActivityType,
    level: ActivityLevel,
    outcome: std::sync::Mutex<ActivityOutcome>,
}

impl Activity {
    /// Create a new Activity (called by builders)
    pub(crate) fn new(
        span: Span,
        id: u64,
        activity_type: ActivityType,
        level: ActivityLevel,
    ) -> Self {
        Self {
            span,
            id,
            activity_type,
            level,
            outcome: std::sync::Mutex::new(ActivityOutcome::Success),
        }
    }

    /// Create a builder for a Build activity
    pub fn build(name: impl Into<String>) -> BuildBuilder {
        BuildBuilder::new(name)
    }

    /// Create a builder for a Fetch activity
    pub fn fetch(kind: FetchKind, name: impl Into<String>) -> FetchBuilder {
        FetchBuilder::new(kind, name)
    }

    /// Create a builder for an Evaluate activity
    pub fn evaluate(name: impl Into<String>) -> EvaluateBuilder {
        EvaluateBuilder::new(name)
    }

    /// Create a builder for a Task activity
    pub fn task() -> TaskBuilder {
        TaskBuilder::new()
    }

    /// Create and start a Task activity with a pre-assigned ID.
    pub fn task_with_id(id: u64) -> Activity {
        Activity::task().id(id).start()
    }

    /// Create a builder for a Command activity
    pub fn command(name: impl Into<String>) -> CommandBuilder {
        CommandBuilder::new(name)
    }

    /// Create a builder for an Operation activity
    pub fn operation(name: impl Into<String>) -> OperationBuilder {
        OperationBuilder::new(name)
    }

    /// Get the activity ID
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Get the activity level
    pub fn level(&self) -> ActivityLevel {
        self.level
    }

    /// Get a cloned span for this activity.
    pub fn span(&self) -> Span {
        self.span.clone()
    }

    /// Run a closure with this activity's context propagated, creating a new task-local scope.
    /// Nested activities created within the closure will see this activity as their parent
    /// and inherit this activity's level by default.
    ///
    /// # Example
    /// ```ignore
    /// let activity = Activity::task().start();
    /// activity.with_new_scope_sync(|| {
    ///     // This child will have `activity` as its parent and inherit its level
    ///     let child = Activity::task().start();
    /// });
    /// ```
    pub fn with_new_scope_sync<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        let mut stack = get_current_stack();
        stack.push((self.id, self.level));
        ACTIVITY_STACK.sync_scope(RefCell::new(stack), f)
    }

    /// Run a synchronous closure within this activity's scope.
    ///
    /// While the closure runs, `current_activity_id()` will return this activity's ID.
    /// Use this for synchronous code like FFI calls. For async code, use `in_activity()`.
    ///
    /// Unlike `with_new_scope_sync`, this modifies the existing task-local stack in-place.
    /// If no task-local stack exists, the closure runs without activity tracking.
    ///
    /// # Example
    /// ```ignore
    /// let activity = Activity::evaluate("Building shell").start();
    /// let result = activity.in_scope(|| {
    ///     // FFI calls here will see this activity as current
    ///     ffi_operation()
    /// });
    /// ```
    pub fn in_scope<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        ACTIVITY_STACK
            .try_with(|stack| {
                stack.borrow_mut().push((self.id, self.level));
            })
            .ok();

        let result = f();

        ACTIVITY_STACK
            .try_with(|stack| {
                stack.borrow_mut().pop();
            })
            .ok();

        result
    }

    /// Mark as failed
    pub fn fail(&self) {
        if let Ok(mut outcome) = self.outcome.lock() {
            *outcome = ActivityOutcome::Failed;
        }
    }

    /// Mark as cancelled
    pub fn cancel(&self) {
        if let Ok(mut outcome) = self.outcome.lock() {
            *outcome = ActivityOutcome::Cancelled;
        }
    }

    /// Mark as cached (task output was already cached)
    pub fn cached(&self) {
        if let Ok(mut outcome) = self.outcome.lock() {
            *outcome = ActivityOutcome::Cached;
        }
    }

    /// Mark as skipped (task had no command to run)
    pub fn skipped(&self) {
        if let Ok(mut outcome) = self.outcome.lock() {
            *outcome = ActivityOutcome::Skipped;
        }
    }

    /// Mark as dependency failed
    pub fn dependency_failed(&self) {
        if let Ok(mut outcome) = self.outcome.lock() {
            *outcome = ActivityOutcome::DependencyFailed;
        }
    }

    /// Update progress (for Build, Task, and Operation activities)
    ///
    /// For Operation activities, an optional detail string can be provided to show
    /// what is currently being processed (e.g., the current file or path name).
    pub fn progress(&self, done: u64, expected: u64, detail: Option<&str>) {
        let _guard = self.span.enter();
        let event = match self.activity_type {
            ActivityType::Build => ActivityEvent::Build(Build::Progress {
                id: self.id,
                done,
                expected,
                timestamp: Timestamp::now(),
            }),
            ActivityType::Task => ActivityEvent::Task(Task::Progress {
                id: self.id,
                done,
                expected,
                timestamp: Timestamp::now(),
            }),
            ActivityType::Fetch(_) => {
                // For fetch, use progress_bytes instead
                return;
            }
            ActivityType::Operation => ActivityEvent::Operation(Operation::Progress {
                id: self.id,
                done,
                expected,
                detail: detail.map(String::from),
                timestamp: Timestamp::now(),
            }),
            _ => return,
        };
        send_activity_event(event);
    }

    /// Update progress with bytes (for Fetch activities)
    pub fn progress_bytes(&self, current: u64, total: u64) {
        let _guard = self.span.enter();
        if matches!(self.activity_type, ActivityType::Fetch(_)) {
            send_activity_event(ActivityEvent::Fetch(Fetch::Progress {
                id: self.id,
                current,
                total: Some(total),
                timestamp: Timestamp::now(),
            }));
        }
    }

    /// Update progress (indeterminate - for Fetch activities)
    pub fn progress_indeterminate(&self, current: u64) {
        let _guard = self.span.enter();
        if matches!(self.activity_type, ActivityType::Fetch(_)) {
            send_activity_event(ActivityEvent::Fetch(Fetch::Progress {
                id: self.id,
                current,
                total: None,
                timestamp: Timestamp::now(),
            }));
        }
    }

    /// Update phase (for Build activities only)
    pub fn phase(&self, phase: impl Into<String>) {
        let _guard = self.span.enter();
        let phase_str = phase.into();
        if matches!(self.activity_type, ActivityType::Build) {
            send_activity_event(ActivityEvent::Build(Build::Phase {
                id: self.id,
                phase: phase_str,
                timestamp: Timestamp::now(),
            }));
        }
    }

    /// Log a line
    pub fn log(&self, line: impl Into<String>) {
        let _guard = self.span.enter();
        let line_str = line.into();
        let event = match self.activity_type {
            ActivityType::Build => ActivityEvent::Build(Build::Log {
                id: self.id,
                line: line_str,
                is_error: false,
                timestamp: Timestamp::now(),
            }),
            ActivityType::Evaluate => ActivityEvent::Evaluate(Evaluate::Log {
                id: self.id,
                line: line_str,
                timestamp: Timestamp::now(),
            }),
            ActivityType::Task => ActivityEvent::Task(Task::Log {
                id: self.id,
                line: line_str,
                is_error: false,
                timestamp: Timestamp::now(),
            }),
            ActivityType::Command => ActivityEvent::Command(Command::Log {
                id: self.id,
                line: line_str,
                is_error: false,
                timestamp: Timestamp::now(),
            }),
            ActivityType::Operation => ActivityEvent::Operation(Operation::Log {
                id: self.id,
                line: line_str,
                is_error: false,
                timestamp: Timestamp::now(),
            }),
            _ => return,
        };
        send_activity_event(event);
    }

    /// Log an error
    pub fn error(&self, line: impl Into<String>) {
        let _guard = self.span.enter();
        let line_str = line.into();
        let event = match self.activity_type {
            ActivityType::Build => ActivityEvent::Build(Build::Log {
                id: self.id,
                line: line_str,
                is_error: true,
                timestamp: Timestamp::now(),
            }),
            ActivityType::Task => ActivityEvent::Task(Task::Log {
                id: self.id,
                line: line_str,
                is_error: true,
                timestamp: Timestamp::now(),
            }),
            ActivityType::Command => ActivityEvent::Command(Command::Log {
                id: self.id,
                line: line_str,
                is_error: true,
                timestamp: Timestamp::now(),
            }),
            ActivityType::Operation => ActivityEvent::Operation(Operation::Log {
                id: self.id,
                line: line_str,
                is_error: true,
                timestamp: Timestamp::now(),
            }),
            _ => return,
        };
        send_activity_event(event);
    }
}

impl Deref for Activity {
    type Target = Span;

    fn deref(&self) -> &Self::Target {
        &self.span
    }
}

impl Clone for Activity {
    fn clone(&self) -> Self {
        let outcome = self
            .outcome
            .lock()
            .map(|o| *o)
            .unwrap_or(ActivityOutcome::Success);
        Self {
            span: self.span.clone(),
            id: self.id,
            activity_type: self.activity_type,
            level: self.level,
            outcome: std::sync::Mutex::new(outcome),
        }
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        let outcome = self
            .outcome
            .lock()
            .map(|o| *o)
            .unwrap_or(ActivityOutcome::Success);

        // Send the correct Complete event based on activity type
        let event = match self.activity_type {
            ActivityType::Build => ActivityEvent::Build(Build::Complete {
                id: self.id,
                outcome,
                timestamp: Timestamp::now(),
            }),
            ActivityType::Fetch(_) => ActivityEvent::Fetch(Fetch::Complete {
                id: self.id,
                outcome,
                timestamp: Timestamp::now(),
            }),
            ActivityType::Evaluate => ActivityEvent::Evaluate(Evaluate::Complete {
                id: self.id,
                outcome,
                timestamp: Timestamp::now(),
            }),
            ActivityType::Task => ActivityEvent::Task(Task::Complete {
                id: self.id,
                outcome,
                timestamp: Timestamp::now(),
            }),
            ActivityType::Command => ActivityEvent::Command(Command::Complete {
                id: self.id,
                outcome,
                timestamp: Timestamp::now(),
            }),
            ActivityType::Operation => ActivityEvent::Operation(Operation::Complete {
                id: self.id,
                outcome,
                timestamp: Timestamp::now(),
            }),
        };
        send_activity_event(event);
    }
}
