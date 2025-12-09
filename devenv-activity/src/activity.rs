//! Activity guard that tracks an activity's lifecycle.

use std::cell::RefCell;
use std::future::Future;
use std::ops::Deref;

use tracing::Span;

use crate::Timestamp;
use crate::builders::{
    BuildBuilder, CommandBuilder, EvaluateBuilder, FetchBuilder, OperationBuilder, TaskBuilder,
};
use crate::events::{
    ActivityEvent, ActivityOutcome, Build, Command, Evaluate, Fetch, FetchKind, Operation, Task,
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
    outcome: std::sync::Mutex<ActivityOutcome>,
}

impl Activity {
    /// Create a new Activity (called by builders)
    pub(crate) fn new(span: Span, id: u64, activity_type: ActivityType) -> Self {
        Self {
            span,
            id,
            activity_type,
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
    pub fn task(name: impl Into<String>) -> TaskBuilder {
        TaskBuilder::new(name)
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

    /// Get a cloned span for this activity.
    pub fn span(&self) -> Span {
        self.span.clone()
    }

    /// Run a future with this activity's context propagated.
    /// Nested activities created within the future will see this activity as their parent.
    ///
    /// # Example
    /// ```ignore
    /// let activity = Activity::task("parent").start();
    /// activity.scope(async {
    ///     // This child will have `activity` as its parent
    ///     let child = Activity::task("child").start();
    /// }).await;
    /// ```
    pub async fn scope<F, T>(&self, f: F) -> T
    where
        F: Future<Output = T>,
    {
        let mut stack = get_current_stack();
        stack.push(self.id);
        ACTIVITY_STACK.scope(RefCell::new(stack), f).await
    }

    /// Run a closure with this activity's context propagated.
    /// Nested activities created within the closure will see this activity as their parent.
    ///
    /// # Example
    /// ```ignore
    /// let activity = Activity::task("parent").start();
    /// activity.scope_sync(|| {
    ///     // This child will have `activity` as its parent
    ///     let child = Activity::task("child").start();
    /// });
    /// ```
    pub fn scope_sync<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        let mut stack = get_current_stack();
        stack.push(self.id);
        ACTIVITY_STACK.sync_scope(RefCell::new(stack), f)
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

    /// Update progress (for Build and Task activities)
    pub fn progress(&self, done: u64, expected: u64) {
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
