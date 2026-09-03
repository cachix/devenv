//! Activity guard that tracks an activity's lifecycle.

use std::cell::RefCell;
use std::ops::Deref;
use std::panic::Location;
use std::sync::Arc;

use tracing::Span;

use crate::Timestamp;
use crate::builders::{
    BuildBuilder, CommandBuilder, EvaluateBuilder, FetchBuilder, OperationBuilder, ProcessBuilder,
    TaskBuilder,
};
use crate::events::{
    ActivityEvent, ActivityLevel, ActivityOutcome, Build, Command, Evaluate, Fetch, FetchKind,
    Operation, Process, ProcessStatus, Task,
};
use crate::stack::{
    ACTIVITY_STACK, activity_sender_installed, get_current_stack, send_activity_event,
};

/// Activity type for tracking which kind of activity this is
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityType {
    Build,
    Fetch(FetchKind),
    Evaluate,
    Task,
    Command,
    Process,
    Operation,
}

/// Create a log event for the given activity type.
///
/// Returns `None` for activity types that do not support logging (e.g. Fetch)
/// or when `is_error` is true for Evaluate (which has no error log variant).
fn make_log_event(
    id: u64,
    activity_type: ActivityType,
    line: String,
    is_error: bool,
) -> Option<ActivityEvent> {
    let timestamp = Timestamp::now();
    let event = match activity_type {
        ActivityType::Build => ActivityEvent::Build(Build::Log {
            id,
            line,
            is_error,
            timestamp,
        }),
        ActivityType::Evaluate => {
            if is_error {
                return None;
            }
            ActivityEvent::Evaluate(Evaluate::Log {
                id,
                line,
                timestamp,
            })
        }
        ActivityType::Task => ActivityEvent::Task(Task::Log {
            id,
            line,
            is_error,
            timestamp,
        }),
        ActivityType::Command => ActivityEvent::Command(Command::Log {
            id,
            line,
            is_error,
            timestamp,
        }),
        ActivityType::Process => ActivityEvent::Process(Process::Log {
            id,
            line,
            is_error,
            timestamp,
        }),
        ActivityType::Operation => ActivityEvent::Operation(Operation::Log {
            id,
            line,
            is_error,
            timestamp,
        }),
        _ => return None,
    };
    Some(event)
}

/// Mirror an activity update into tracing under its span, then send it over
/// the activity channel.
///
/// The tracing mirror only runs when an exporter enabled the
/// `devenv_activity::events` target, and it borrows the event rather than
/// serializing it. The channel always receives the typed event by value.
fn emit(span: &Span, event: ActivityEvent, caller: &'static Location<'static>) {
    crate::__trace_activity_event!(parent: span, &event, caller);
    send_activity_event(event);
}

/// Create a complete event for the given activity type.
fn make_complete_event(
    id: u64,
    activity_type: ActivityType,
    outcome: ActivityOutcome,
) -> ActivityEvent {
    let timestamp = Timestamp::now();
    match activity_type {
        ActivityType::Build => ActivityEvent::Build(Build::Complete {
            id,
            outcome,
            timestamp,
        }),
        ActivityType::Fetch(_) => ActivityEvent::Fetch(Fetch::Complete {
            id,
            outcome,
            timestamp,
        }),
        ActivityType::Evaluate => ActivityEvent::Evaluate(Evaluate::Complete {
            id,
            outcome,
            timestamp,
        }),
        ActivityType::Task => ActivityEvent::Task(Task::Complete {
            id,
            outcome,
            timestamp,
        }),
        ActivityType::Command => ActivityEvent::Command(Command::Complete {
            id,
            outcome,
            timestamp,
        }),
        ActivityType::Process => ActivityEvent::Process(Process::Complete {
            id,
            outcome,
            timestamp,
        }),
        ActivityType::Operation => ActivityEvent::Operation(Operation::Complete {
            id,
            outcome,
            timestamp,
        }),
    }
}

/// Guard that tracks an activity's lifecycle via tracing spans.
/// Activity is Send + Sync, allowing storage in Mutex for async callbacks.
#[must_use = "Activity will complete immediately if dropped"]
pub struct Activity {
    span: Span,
    id: u64,
    activity_type: ActivityType,
    level: ActivityLevel,
    outcome: Arc<std::sync::Mutex<ActivityOutcome>>,
    complete_on_drop: bool,
    /// Where the activity was started. The completion event emitted on drop
    /// is attributed to this location.
    caller: &'static Location<'static>,
}

impl Activity {
    /// Create a new Activity (called by builders)
    pub(crate) fn new(
        span: Span,
        id: u64,
        activity_type: ActivityType,
        level: ActivityLevel,
        caller: &'static Location<'static>,
    ) -> Self {
        Self {
            span,
            id,
            activity_type,
            level,
            outcome: Arc::new(std::sync::Mutex::new(ActivityOutcome::Success)),
            complete_on_drop: true,
            caller,
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

    /// Create a builder for a Process activity (long-running managed process)
    pub fn process(name: impl Into<String>) -> ProcessBuilder {
        ProcessBuilder::new(name)
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

        let result = self.span.in_scope(f);

        ACTIVITY_STACK
            .try_with(|stack| {
                stack.borrow_mut().pop();
            })
            .ok();

        result
    }

    /// Run a synchronous operation in this activity and complete the activity
    /// as soon as the operation returns.
    ///
    /// Unlike [`in_scope`](Self::in_scope), this consumes the owning activity,
    /// so it cannot accidentally remain active during a later phase.
    pub fn scoped<F, R>(self, f: F) -> R
    where
        F: FnOnce(&Self) -> R,
    {
        self.in_scope(|| f(&self))
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

    /// Reset outcome to success (for restarting failed processes)
    pub fn reset(&self) {
        if let Ok(mut outcome) = self.outcome.lock() {
            *outcome = ActivityOutcome::Success;
        }
    }

    /// Update progress (for Build, Task, and Operation activities)
    ///
    /// For Operation activities, an optional detail string can be provided to show
    /// what is currently being processed (e.g., the current file or path name).
    #[track_caller]
    pub fn progress(&self, done: u64, expected: u64, detail: Option<&str>) {
        let caller = std::panic::Location::caller();
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
        emit(&self.span, event, caller);
    }

    /// Update progress with bytes (for Fetch activities)
    #[track_caller]
    pub fn progress_bytes(&self, current: u64, total: u64) {
        let caller = std::panic::Location::caller();
        if matches!(self.activity_type, ActivityType::Fetch(_)) {
            let event = ActivityEvent::Fetch(Fetch::Progress {
                id: self.id,
                current,
                total: Some(total),
                timestamp: Timestamp::now(),
            });
            emit(&self.span, event, caller);
        }
    }

    /// Update progress (indeterminate - for Fetch activities)
    #[track_caller]
    pub fn progress_indeterminate(&self, current: u64) {
        let caller = std::panic::Location::caller();
        if matches!(self.activity_type, ActivityType::Fetch(_)) {
            let event = ActivityEvent::Fetch(Fetch::Progress {
                id: self.id,
                current,
                total: None,
                timestamp: Timestamp::now(),
            });
            emit(&self.span, event, caller);
        }
    }

    /// Update phase (for Build activities only)
    #[track_caller]
    pub fn phase(&self, phase: impl Into<String>) {
        let caller = std::panic::Location::caller();
        let phase_str = phase.into();
        if matches!(self.activity_type, ActivityType::Build) {
            let event = ActivityEvent::Build(Build::Phase {
                id: self.id,
                phase: phase_str,
                timestamp: Timestamp::now(),
            });
            emit(&self.span, event, caller);
        }
    }

    /// Log a line
    #[track_caller]
    pub fn log(&self, line: impl Into<String>) {
        let caller = std::panic::Location::caller();
        let line_str = line.into();
        if !activity_sender_installed() {
            self.span.in_scope(|| tracing::info!("{}", line_str));
        }
        if let Some(event) = make_log_event(self.id, self.activity_type, line_str, false) {
            emit(&self.span, event, caller);
        }
    }

    /// Log an error
    #[track_caller]
    pub fn error(&self, line: impl Into<String>) {
        let caller = std::panic::Location::caller();
        let line_str = line.into();
        if !activity_sender_installed() {
            self.span.in_scope(|| tracing::warn!("{}", line_str));
        }
        if let Some(event) = make_log_event(self.id, self.activity_type, line_str, true) {
            emit(&self.span, event, caller);
        }
    }

    /// Set process status (for Process activities only)
    #[track_caller]
    pub fn set_status(&self, status: ProcessStatus) {
        let caller = std::panic::Location::caller();
        if matches!(self.activity_type, ActivityType::Process) {
            self.span.record("devenv.process.status", status.as_str());
            let event = ActivityEvent::Process(Process::Status {
                id: self.id,
                status,
                timestamp: Timestamp::now(),
            });
            emit(&self.span, event, caller);
        }
    }

    /// Record that the process exited (for Process activities only)
    #[track_caller]
    pub fn exited(&self, success: bool) {
        let caller = std::panic::Location::caller();
        if matches!(self.activity_type, ActivityType::Process) {
            let event = ActivityEvent::Process(Process::Exited {
                id: self.id,
                success,
                timestamp: Timestamp::now(),
            });
            emit(&self.span, event, caller);
        }
    }

    /// Record that the supervisor restarted the process (for Process activities only)
    #[track_caller]
    pub fn restarted(&self, attempt: u64) {
        let caller = std::panic::Location::caller();
        if matches!(self.activity_type, ActivityType::Process) {
            let event = ActivityEvent::Process(Process::Restarted {
                id: self.id,
                attempt,
                timestamp: Timestamp::now(),
            });
            emit(&self.span, event, caller);
        }
    }

    /// Create a non-owning reference handle.
    ///
    /// The returned `ActivityRef` shares the outcome with this `Activity`
    /// and can log, set status, and mutate the outcome, but does NOT send
    /// a `Complete` event when dropped.
    pub fn ref_handle(&self) -> ActivityRef {
        ActivityRef {
            span: self.span.clone(),
            id: self.id,
            activity_type: self.activity_type,
            outcome: self.outcome.clone(),
        }
    }

    /// Convert this owning activity into a non-owning reference.
    ///
    /// This is used when mirroring an activity owned by another process. The
    /// local guard relinquishes completion ownership before it is dropped, so
    /// disconnecting the observer cannot emit a synthetic `Complete` event.
    pub fn into_ref(mut self) -> ActivityRef {
        let activity_ref = self.ref_handle();
        self.complete_on_drop = false;
        activity_ref
    }
}

/// Non-owning handle to an activity.
///
/// Can log, set status, and mutate the shared outcome, but does NOT send
/// a `Complete` event on drop (unlike `Activity`).
#[derive(Clone)]
pub struct ActivityRef {
    span: Span,
    id: u64,
    activity_type: ActivityType,
    outcome: Arc<std::sync::Mutex<ActivityOutcome>>,
}

impl ActivityRef {
    /// Log a line
    #[track_caller]
    pub fn log(&self, line: impl Into<String>) {
        let caller = std::panic::Location::caller();
        let line_str = line.into();
        if !activity_sender_installed() {
            self.span.in_scope(|| tracing::info!("{}", line_str));
        }
        if let Some(event) = make_log_event(self.id, self.activity_type, line_str, false) {
            emit(&self.span, event, caller);
        }
    }

    /// Log an error
    #[track_caller]
    pub fn error(&self, line: impl Into<String>) {
        let caller = std::panic::Location::caller();
        let line_str = line.into();
        if !activity_sender_installed() {
            self.span.in_scope(|| tracing::warn!("{}", line_str));
        }
        if let Some(event) = make_log_event(self.id, self.activity_type, line_str, true) {
            emit(&self.span, event, caller);
        }
    }

    /// Set process status (for Process activities only)
    #[track_caller]
    pub fn set_status(&self, status: ProcessStatus) {
        let caller = std::panic::Location::caller();
        if matches!(self.activity_type, ActivityType::Process) {
            self.span.record("devenv.process.status", status.as_str());
            let event = ActivityEvent::Process(Process::Status {
                id: self.id,
                status,
                timestamp: Timestamp::now(),
            });
            emit(&self.span, event, caller);
        }
    }

    /// Record that the process exited (for Process activities only)
    #[track_caller]
    pub fn exited(&self, success: bool) {
        let caller = std::panic::Location::caller();
        if matches!(self.activity_type, ActivityType::Process) {
            let event = ActivityEvent::Process(Process::Exited {
                id: self.id,
                success,
                timestamp: Timestamp::now(),
            });
            emit(&self.span, event, caller);
        }
    }

    /// Record that the supervisor restarted the process (for Process activities only)
    #[track_caller]
    pub fn restarted(&self, attempt: u64) {
        let caller = std::panic::Location::caller();
        if matches!(self.activity_type, ActivityType::Process) {
            let event = ActivityEvent::Process(Process::Restarted {
                id: self.id,
                attempt,
                timestamp: Timestamp::now(),
            });
            emit(&self.span, event, caller);
        }
    }

    /// Mark as failed
    pub fn fail(&self) {
        if let Ok(mut outcome) = self.outcome.lock() {
            *outcome = ActivityOutcome::Failed;
        }
    }

    /// Reset outcome to success (for restarting failed processes)
    pub fn reset(&self) {
        if let Ok(mut outcome) = self.outcome.lock() {
            *outcome = ActivityOutcome::Success;
        }
    }
}

impl Deref for ActivityRef {
    type Target = Span;

    fn deref(&self) -> &Self::Target {
        &self.span
    }
}

impl Deref for Activity {
    type Target = Span;

    fn deref(&self) -> &Self::Target {
        &self.span
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        if !self.complete_on_drop {
            return;
        }

        let outcome = self
            .outcome
            .lock()
            .map(|o| *o)
            .unwrap_or(ActivityOutcome::Success);

        self.span.record("devenv.outcome", outcome.as_str());
        if outcome.is_error() {
            self.span.record("otel.status_code", "ERROR");
        }
        // This explicit transition, rather than the final tracing span close,
        // preserves ActivityRef semantics: borrowed span handles may outlive
        // the owning activity, and into_ref() deliberately has no completion.
        self.span.record("devenv.activity.complete", true);

        emit(
            &self.span,
            make_complete_event(self.id, self.activity_type, outcome),
            self.caller,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{ActivityEvent, Operation, Process};

    fn process_complete_id(event: &ActivityEvent) -> Option<u64> {
        match event {
            ActivityEvent::Process(Process::Complete { id, .. }) => Some(*id),
            _ => None,
        }
    }

    #[test]
    fn owning_and_proxy_activity_lifecycles_are_distinct() {
        // This one test owns the process-global activity sender for its full
        // duration. Keeping the lifecycle cases together prevents these
        // assertions from racing one another.
        let (mut rx, handle) = crate::init();
        let _guard = handle.install();

        let owner = crate::start!(Activity::process("owner"));
        let owner_id = owner.id();
        drop(owner);
        let owner_events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert_eq!(
            owner_events
                .iter()
                .filter(|event| process_complete_id(event) == Some(owner_id))
                .count(),
            1,
            "dropping an owning activity must emit exactly one completion"
        );

        let owner = crate::start!(Activity::process("borrowed"));
        let borrowed_id = owner.id();
        let borrowed = owner.ref_handle();
        borrowed.log("still observable");
        borrowed.set_status(ProcessStatus::Ready);
        drop(borrowed);
        let borrowed_events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(
            borrowed_events
                .iter()
                .all(|event| process_complete_id(event) != Some(borrowed_id)),
            "dropping an ActivityRef must not complete its owner"
        );
        drop(owner);
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok())
                .any(|event| process_complete_id(&event) == Some(borrowed_id)),
            "the owning guard must retain completion ownership"
        );

        let scoped = crate::start!(Activity::operation("scoped"));
        let scoped_id = scoped.id();
        let result = scoped.scoped(|activity| {
            activity.progress(1, 1, None);
            42
        });
        assert_eq!(result, 42);
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok()).any(|event| matches!(
                event,
                ActivityEvent::Operation(Operation::Complete { id, .. }) if id == scoped_id
            ))
        );

        let proxy = crate::start!(Activity::process("proxy")).into_ref();
        let proxy_id = proxy.id;
        proxy.fail();
        proxy.set_status(ProcessStatus::Stopped);
        proxy.reset();
        proxy.set_status(ProcessStatus::Running);
        proxy.log("live after terminal status");
        drop(proxy);

        let proxy_events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(
            proxy_events
                .iter()
                .all(|event| process_complete_id(event) != Some(proxy_id)),
            "converting an owner into a proxy must relinquish completion ownership"
        );
        assert!(
            proxy_events.iter().any(|event| matches!(
                event,
                ActivityEvent::Process(Process::Status {
                    id,
                    status: ProcessStatus::Running,
                    ..
                }) if *id == proxy_id
            )),
            "a proxy must accept status updates after a terminal status"
        );
        assert!(
            proxy_events.iter().any(|event| matches!(
                event,
                ActivityEvent::Process(Process::Log { id, line, .. })
                    if *id == proxy_id && line == "live after terminal status"
            )),
            "proxy log updates must remain observable"
        );
    }
}
