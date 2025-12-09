//! Activity tracking system for devenv built on tracing.
//!
//! This crate provides a unified activity tracking system that:
//! - Uses typed events as the single source of truth
//! - Supports multiple consumers via tracing's layer system
//! - Provides automatic context propagation via span hierarchy
//! - Offers zero-cost filtering via tracing's infrastructure
//!
//! ## Usage
//!
//! Use the `ActivityInstrument` trait to instrument async code with activities:
//!
//! ```ignore
//! use devenv_activity::{Activity, ActivityInstrument};
//!
//! let activity = Activity::operation("Building").start();
//! async {
//!     // Nested activities will have `activity` as their parent
//! }
//! .in_activity(&activity)
//! .await;
//! ```
//!
//! ## Using the `#[activity]` macro
//!
//! For cleaner instrumentation, use the `#[activity]` attribute macro:
//!
//! ```ignore
//! use devenv_activity::activity;
//!
//! #[activity("Building shell")]
//! async fn build_shell() -> Result<()> {
//!     // Function body is automatically instrumented
//!     Ok(())
//! }
//! ```

// Re-export the activity macro
pub use devenv_activity_macros::activity;

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::future::Future;
use std::ops::Deref;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;
use tokio::sync::mpsc;
use tracing::{Level, Span, span};

// Re-export for convenience
pub use tracing_subscriber::Registry;

// Global sender for activity events (installed by ActivityHandle::install())
static ACTIVITY_SENDER: OnceLock<mpsc::Sender<ActivityEvent>> = OnceLock::new();

/// Send an activity event to the registered channel and emit to tracing
fn send_activity_event(event: ActivityEvent) {
    // Emit to tracing for file export - serialize as JSON string
    if let Ok(json) = serde_json::to_string(&event) {
        tracing::trace!(target: "devenv::activity", event = json);
    }

    // Send to channel for TUI (non-blocking)
    if let Some(tx) = ACTIVITY_SENDER.get() {
        let _ = tx.try_send(event);
    }
}

/// RFC 3339 timestamp wrapper for SystemTime with proper serde serialization
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp(pub SystemTime);

impl Timestamp {
    pub fn now() -> Self {
        Self(SystemTime::now())
    }
}

impl From<SystemTime> for Timestamp {
    fn from(time: SystemTime) -> Self {
        Self(time)
    }
}

impl Serialize for Timestamp {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&humantime::format_rfc3339_nanos(self.0).to_string())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let s = String::deserialize(deserializer)?;
        humantime::parse_rfc3339(&s)
            .map(Timestamp)
            .map_err(D::Error::custom)
    }
}

// Task-local stack for tracking current Activity IDs (for parent detection).
// Using task_local instead of thread_local to support async code where tasks
// can migrate between threads via Tokio's work-stealing scheduler.
tokio::task_local! {
    static ACTIVITY_STACK: RefCell<Vec<u64>>;
}

/// All activity events - activity-first design
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "activity_kind", rename_all = "lowercase")]
pub enum ActivityEvent {
    Build(Build),
    Fetch(Fetch),
    Evaluate(Evaluate),
    Task(Task),
    Command(Command),
    Operation(Operation),
    Message(Message),
}

/// Build activity events - has Phase, Progress, Log
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Build {
    Start {
        #[serde(alias = "activity_id")]
        id: u64,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        derivation_path: Option<String>,
        timestamp: Timestamp,
    },
    Complete {
        #[serde(alias = "activity_id")]
        id: u64,
        outcome: ActivityOutcome,
        timestamp: Timestamp,
    },
    Phase {
        #[serde(alias = "activity_id")]
        id: u64,
        phase: String,
        timestamp: Timestamp,
    },
    Progress {
        #[serde(alias = "activity_id")]
        id: u64,
        done: u64,
        expected: u64,
        timestamp: Timestamp,
    },
    Log {
        #[serde(alias = "activity_id")]
        id: u64,
        line: String,
        #[serde(default)]
        is_error: bool,
        timestamp: Timestamp,
    },
}

/// Fetch activity events - has FetchKind, byte Progress
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Fetch {
    Start {
        #[serde(alias = "activity_id")]
        id: u64,
        kind: FetchKind,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        timestamp: Timestamp,
    },
    Complete {
        #[serde(alias = "activity_id")]
        id: u64,
        outcome: ActivityOutcome,
        timestamp: Timestamp,
    },
    Progress {
        #[serde(alias = "activity_id")]
        id: u64,
        current: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<u64>,
        timestamp: Timestamp,
    },
}

/// Type of fetch operation
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FetchKind {
    /// Downloading store paths from substituter
    Download,
    /// Querying path info from cache
    Query,
    /// Fetching git trees/flake inputs
    Tree,
}

/// Evaluate activity events - has Log only
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Evaluate {
    Start {
        #[serde(alias = "activity_id")]
        id: u64,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<u64>,
        timestamp: Timestamp,
    },
    Complete {
        #[serde(alias = "activity_id")]
        id: u64,
        outcome: ActivityOutcome,
        timestamp: Timestamp,
    },
    Log {
        #[serde(alias = "activity_id")]
        id: u64,
        line: String,
        timestamp: Timestamp,
    },
}

/// Task activity events - has Progress, Log
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Task {
    Start {
        #[serde(alias = "activity_id")]
        id: u64,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        timestamp: Timestamp,
    },
    Complete {
        #[serde(alias = "activity_id")]
        id: u64,
        outcome: ActivityOutcome,
        timestamp: Timestamp,
    },
    Progress {
        #[serde(alias = "activity_id")]
        id: u64,
        done: u64,
        expected: u64,
        timestamp: Timestamp,
    },
    Log {
        #[serde(alias = "activity_id")]
        id: u64,
        line: String,
        #[serde(default)]
        is_error: bool,
        timestamp: Timestamp,
    },
}

/// Command activity events - has Log only
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Command {
    Start {
        #[serde(alias = "activity_id")]
        id: u64,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        timestamp: Timestamp,
    },
    Complete {
        #[serde(alias = "activity_id")]
        id: u64,
        outcome: ActivityOutcome,
        timestamp: Timestamp,
    },
    Log {
        #[serde(alias = "activity_id")]
        id: u64,
        line: String,
        #[serde(default)]
        is_error: bool,
        timestamp: Timestamp,
    },
}

/// Operation activity events - minimal (generic devenv operations)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Operation {
    Start {
        #[serde(alias = "activity_id")]
        id: u64,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        timestamp: Timestamp,
    },
    Complete {
        #[serde(alias = "activity_id")]
        id: u64,
        outcome: ActivityOutcome,
        timestamp: Timestamp,
    },
}

/// Message - standalone (not an activity)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub level: ActivityLevel,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    pub timestamp: Timestamp,
}

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

/// Outcome of an activity
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActivityOutcome {
    #[default]
    Success,
    Failed,
    Cancelled,
}

/// Activity level (maps to tracing::Level)
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    strum::EnumString,
    strum::Display,
    serde_with::DeserializeFromStr,
    serde_with::SerializeDisplay,
)]
#[strum(serialize_all = "snake_case")]
pub enum ActivityLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

// ---------------------------------------------------------------------------
// ID Generation
// ---------------------------------------------------------------------------

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Generate a new activity ID.
/// Uses high bit to distinguish from Nix-generated IDs.
fn next_id() -> u64 {
    ID_COUNTER.fetch_add(1, Ordering::Relaxed) | (1 << 63)
}

// ---------------------------------------------------------------------------
// Activity Builders
// ---------------------------------------------------------------------------

/// Builder for Build activities
pub struct BuildBuilder {
    name: String,
    derivation_path: Option<String>,
    id: Option<u64>,
    parent: Option<Option<u64>>,
    level: ActivityLevel,
}

impl BuildBuilder {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            derivation_path: None,
            id: None,
            parent: None,
            level: ActivityLevel::default(),
        }
    }

    pub fn derivation_path(mut self, path: impl Into<String>) -> Self {
        self.derivation_path = Some(path.into());
        self
    }

    pub fn id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    pub fn parent(mut self, parent: Option<u64>) -> Self {
        self.parent = Some(parent);
        self
    }

    pub fn level(mut self, level: ActivityLevel) -> Self {
        self.level = level;
        self
    }

    pub fn start(self) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let parent = self.parent.unwrap_or_else(current_activity_id);

        let span = create_span(id, self.level);

        send_activity_event(ActivityEvent::Build(Build::Start {
            id,
            name: self.name.clone(),
            parent,
            derivation_path: self.derivation_path,
            timestamp: Timestamp::now(),
        }));

        Activity {
            span,
            id,
            activity_type: ActivityType::Build,
            outcome: std::sync::Mutex::new(ActivityOutcome::Success),
        }
    }
}

/// Builder for Fetch activities
pub struct FetchBuilder {
    kind: FetchKind,
    name: String,
    url: Option<String>,
    id: Option<u64>,
    parent: Option<Option<u64>>,
    level: ActivityLevel,
}

impl FetchBuilder {
    fn new(kind: FetchKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            url: None,
            id: None,
            parent: None,
            level: ActivityLevel::default(),
        }
    }

    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    pub fn id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    pub fn parent(mut self, parent: Option<u64>) -> Self {
        self.parent = Some(parent);
        self
    }

    pub fn level(mut self, level: ActivityLevel) -> Self {
        self.level = level;
        self
    }

    pub fn start(self) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let parent = self.parent.unwrap_or_else(current_activity_id);

        let span = create_span(id, self.level);

        send_activity_event(ActivityEvent::Fetch(Fetch::Start {
            id,
            kind: self.kind,
            name: self.name.clone(),
            parent,
            url: self.url,
            timestamp: Timestamp::now(),
        }));

        Activity {
            span,
            id,
            activity_type: ActivityType::Fetch(self.kind),
            outcome: std::sync::Mutex::new(ActivityOutcome::Success),
        }
    }
}

/// Builder for Evaluate activities
pub struct EvaluateBuilder {
    name: String,
    id: Option<u64>,
    parent: Option<Option<u64>>,
    level: ActivityLevel,
}

impl EvaluateBuilder {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            id: None,
            parent: None,
            level: ActivityLevel::default(),
        }
    }

    pub fn id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    pub fn parent(mut self, parent: Option<u64>) -> Self {
        self.parent = Some(parent);
        self
    }

    pub fn level(mut self, level: ActivityLevel) -> Self {
        self.level = level;
        self
    }

    pub fn start(self) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let parent = self.parent.unwrap_or_else(current_activity_id);

        let span = create_span(id, self.level);

        send_activity_event(ActivityEvent::Evaluate(Evaluate::Start {
            id,
            name: self.name.clone(),
            parent,
            timestamp: Timestamp::now(),
        }));

        Activity {
            span,
            id,
            activity_type: ActivityType::Evaluate,
            outcome: std::sync::Mutex::new(ActivityOutcome::Success),
        }
    }
}

/// Builder for Task activities
pub struct TaskBuilder {
    name: String,
    detail: Option<String>,
    id: Option<u64>,
    parent: Option<Option<u64>>,
    level: ActivityLevel,
}

impl TaskBuilder {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            detail: None,
            id: None,
            parent: None,
            level: ActivityLevel::default(),
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    pub fn parent(mut self, parent: Option<u64>) -> Self {
        self.parent = Some(parent);
        self
    }

    pub fn level(mut self, level: ActivityLevel) -> Self {
        self.level = level;
        self
    }

    pub fn start(self) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let parent = self.parent.unwrap_or_else(current_activity_id);

        let span = create_span(id, self.level);

        send_activity_event(ActivityEvent::Task(Task::Start {
            id,
            name: self.name.clone(),
            parent,
            detail: self.detail,
            timestamp: Timestamp::now(),
        }));

        Activity {
            span,
            id,
            activity_type: ActivityType::Task,
            outcome: std::sync::Mutex::new(ActivityOutcome::Success),
        }
    }
}

/// Builder for Command activities
pub struct CommandBuilder {
    name: String,
    command: Option<String>,
    id: Option<u64>,
    parent: Option<Option<u64>>,
    level: ActivityLevel,
}

impl CommandBuilder {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: None,
            id: None,
            parent: None,
            level: ActivityLevel::Debug, // Commands default to DEBUG level
        }
    }

    pub fn command(mut self, cmd: impl Into<String>) -> Self {
        self.command = Some(cmd.into());
        self
    }

    pub fn id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    pub fn parent(mut self, parent: Option<u64>) -> Self {
        self.parent = Some(parent);
        self
    }

    pub fn level(mut self, level: ActivityLevel) -> Self {
        self.level = level;
        self
    }

    pub fn start(self) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let parent = self.parent.unwrap_or_else(current_activity_id);

        let span = create_span(id, self.level);

        send_activity_event(ActivityEvent::Command(Command::Start {
            id,
            name: self.name.clone(),
            parent,
            command: self.command,
            timestamp: Timestamp::now(),
        }));

        Activity {
            span,
            id,
            activity_type: ActivityType::Command,
            outcome: std::sync::Mutex::new(ActivityOutcome::Success),
        }
    }
}

/// Builder for Operation activities
pub struct OperationBuilder {
    name: String,
    detail: Option<String>,
    id: Option<u64>,
    parent: Option<Option<u64>>,
    level: ActivityLevel,
}

impl OperationBuilder {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            detail: None,
            id: None,
            parent: None,
            level: ActivityLevel::default(),
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    pub fn parent(mut self, parent: Option<u64>) -> Self {
        self.parent = Some(parent);
        self
    }

    pub fn level(mut self, level: ActivityLevel) -> Self {
        self.level = level;
        self
    }

    pub fn start(self) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let parent = self.parent.unwrap_or_else(current_activity_id);

        let span = create_span(id, self.level);

        send_activity_event(ActivityEvent::Operation(Operation::Start {
            id,
            name: self.name.clone(),
            parent,
            detail: self.detail,
            timestamp: Timestamp::now(),
        }));

        Activity {
            span,
            id,
            activity_type: ActivityType::Operation,
            outcome: std::sync::Mutex::new(ActivityOutcome::Success),
        }
    }
}

/// Helper to create a span at the given level
fn create_span(id: u64, level: ActivityLevel) -> Span {
    match level {
        ActivityLevel::Error => span!(Level::ERROR, "activity", activity_id = id),
        ActivityLevel::Warn => span!(Level::WARN, "activity", activity_id = id),
        ActivityLevel::Info => span!(Level::INFO, "activity", activity_id = id),
        ActivityLevel::Debug => span!(Level::DEBUG, "activity", activity_id = id),
        ActivityLevel::Trace => span!(Level::TRACE, "activity", activity_id = id),
    }
}

// ---------------------------------------------------------------------------
// Activity Guard
// ---------------------------------------------------------------------------

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

/// Get the activity ID from the current activity stack.
/// Returns None if not in an activity scope or if the stack is empty.
///
/// Use this to capture the current activity ID before crossing thread/task boundaries,
/// then pass it explicitly to activities created in other contexts.
pub fn current_activity_id() -> Option<u64> {
    ACTIVITY_STACK
        .try_with(|stack| stack.borrow().last().copied())
        .ok()
        .flatten()
}

/// Get a clone of the current activity stack.
/// Returns empty vec if not in an activity scope.
fn get_current_stack() -> Vec<u64> {
    ACTIVITY_STACK
        .try_with(|stack| stack.borrow().clone())
        .unwrap_or_default()
}

/// Emit a standalone message, associated with the current activity if one exists
pub fn message(level: ActivityLevel, text: impl Into<String>) {
    message_with_details(level, text, None)
}

/// Emit a standalone message with optional details, associated with the current activity if one exists
pub fn message_with_details(
    level: ActivityLevel,
    text: impl Into<String>,
    details: Option<String>,
) {
    let text_str = text.into();
    let parent = current_activity_id();
    send_activity_event(ActivityEvent::Message(Message {
        level,
        text: text_str,
        details,
        parent,
        timestamp: Timestamp::now(),
    }));
}
// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Handle for registering the activity event channel
pub struct ActivityHandle {
    tx: mpsc::Sender<ActivityEvent>,
}

impl ActivityHandle {
    /// Install this handle's sender as the global activity event channel.
    /// After calling this, all Activity events will be sent to this channel.
    pub fn install(self) {
        let _ = ACTIVITY_SENDER.set(self.tx);
    }
}

/// Initialize the activity system.
/// Returns receiver for TUI and a handle for installing the channel.
///
/// Usage:
/// ```rust,ignore
/// let (rx, handle) = devenv_activity::init();
/// handle.install();  // Activities now send to this channel
/// // Pass rx to TUI
/// ```
pub fn init() -> (mpsc::Receiver<ActivityEvent>, ActivityHandle) {
    let (tx, rx) = mpsc::channel(32);
    (rx, ActivityHandle { tx })
}

// ---------------------------------------------------------------------------
// Instrument trait
// ---------------------------------------------------------------------------

/// Extension trait for instrumenting futures with activity context.
///
/// This trait provides an `in_activity` method that propagates both:
/// - The activity stack (so nested activities see the activity as their parent)
/// - The tracing span (for tracing instrumentation)
///
/// # Example
/// ```ignore
/// use devenv_activity::{Activity, ActivityInstrument};
///
/// let activity = Activity::operation("Building").start();
/// async {
///     // Nested activities will have `activity` as their parent
///     let child = Activity::task("child").start();
/// }
/// .in_activity(&activity)
/// .await;
/// ```
pub trait ActivityInstrument: Future + Sized {
    /// Instrument this future with the given activity's context.
    fn in_activity(self, activity: &Activity) -> impl Future<Output = Self::Output> {
        let mut stack = get_current_stack();
        stack.push(activity.id);
        let span = activity.span.clone();

        ACTIVITY_STACK.scope(
            RefCell::new(stack),
            tracing::Instrument::instrument(self, span),
        )
    }
}

impl<F: Future> ActivityInstrument for F {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_event_serialization() {
        let event = ActivityEvent::Build(Build::Start {
            id: 123,
            name: "test-package".to_string(),
            parent: Some(456),
            derivation_path: Some("/nix/store/abc-test.drv".to_string()),
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
        });

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""activity_kind":"build"#));
        assert!(json.contains(r#""event":"start"#));

        let parsed: ActivityEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ActivityEvent::Build(Build::Start { id, name, .. }) => {
                assert_eq!(id, 123);
                assert_eq!(name, "test-package");
            }
            _ => panic!("Expected Build::Start event"),
        }
    }

    #[test]
    fn test_fetch_event_with_kind() {
        let event = ActivityEvent::Fetch(Fetch::Start {
            id: 456,
            kind: FetchKind::Download,
            name: "pkg".to_string(),
            parent: None,
            url: Some("https://cache.nixos.org".to_string()),
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
        });

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""activity_kind":"fetch"#));
        assert!(json.contains(r#""kind":"download"#));

        let parsed: ActivityEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ActivityEvent::Fetch(Fetch::Start { kind, .. }) => {
                assert_eq!(kind, FetchKind::Download);
            }
            _ => panic!("Expected Fetch::Start event"),
        }
    }

    #[test]
    fn test_fetch_kinds() {
        let kinds = [FetchKind::Download, FetchKind::Query, FetchKind::Tree];
        for kind in kinds {
            let event = ActivityEvent::Fetch(Fetch::Start {
                id: 1,
                kind,
                name: "test".to_string(),
                parent: None,
                url: None,
                timestamp: Timestamp(SystemTime::UNIX_EPOCH),
            });

            let json = serde_json::to_string(&event).unwrap();
            let parsed: ActivityEvent = serde_json::from_str(&json).unwrap();
            match parsed {
                ActivityEvent::Fetch(Fetch::Start {
                    kind: parsed_kind, ..
                }) => {
                    assert_eq!(parsed_kind, kind);
                }
                _ => panic!("Expected Fetch::Start"),
            }
        }
    }

    #[test]
    fn test_build_complete_event() {
        let event = ActivityEvent::Build(Build::Complete {
            id: 789,
            outcome: ActivityOutcome::Success,
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
        });

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"complete"#));
        assert!(json.contains(r#""outcome":"success"#));

        let parsed: ActivityEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ActivityEvent::Build(Build::Complete { id, outcome, .. }) => {
                assert_eq!(id, 789);
                assert_eq!(outcome, ActivityOutcome::Success);
            }
            _ => panic!("Expected Build::Complete event"),
        }
    }

    #[test]
    fn test_build_phase_event() {
        let event = ActivityEvent::Build(Build::Phase {
            id: 111,
            phase: "configure".to_string(),
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
        });

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"phase"#));

        let parsed: ActivityEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ActivityEvent::Build(Build::Phase { phase, .. }) => {
                assert_eq!(phase, "configure");
            }
            _ => panic!("Expected Build::Phase event"),
        }
    }

    #[test]
    fn test_fetch_progress_event() {
        let event = ActivityEvent::Fetch(Fetch::Progress {
            id: 999,
            current: 50,
            total: Some(100),
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
        });

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"progress"#));
        assert!(json.contains(r#""current":50"#));
        assert!(json.contains(r#""total":100"#));

        let parsed: ActivityEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ActivityEvent::Fetch(Fetch::Progress { current, total, .. }) => {
                assert_eq!(current, 50);
                assert_eq!(total, Some(100));
            }
            _ => panic!("Expected Fetch::Progress event"),
        }
    }

    #[test]
    fn test_message_event() {
        let event = ActivityEvent::Message(Message {
            level: ActivityLevel::Info,
            text: "Test message".to_string(),
            details: None,
            parent: None,
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
        });

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""activity_kind":"message"#));
        assert!(json.contains(r#""level":"info"#));

        let parsed: ActivityEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ActivityEvent::Message(msg) => {
                assert_eq!(msg.level, ActivityLevel::Info);
                assert_eq!(msg.text, "Test message");
            }
            _ => panic!("Expected Message event"),
        }
    }

    #[test]
    fn test_evaluate_log_event() {
        let event = ActivityEvent::Evaluate(Evaluate::Log {
            id: 222,
            line: "Evaluating file...".to_string(),
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
        });

        let json = serde_json::to_string(&event).unwrap();
        let parsed: ActivityEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ActivityEvent::Evaluate(Evaluate::Log { line, .. }) => {
                assert_eq!(line, "Evaluating file...");
            }
            _ => panic!("Expected Evaluate::Log event"),
        }
    }

    #[test]
    fn test_task_progress_event() {
        let event = ActivityEvent::Task(Task::Progress {
            id: 333,
            done: 5,
            expected: 10,
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
        });

        let json = serde_json::to_string(&event).unwrap();
        let parsed: ActivityEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ActivityEvent::Task(Task::Progress { done, expected, .. }) => {
                assert_eq!(done, 5);
                assert_eq!(expected, 10);
            }
            _ => panic!("Expected Task::Progress event"),
        }
    }

    #[test]
    fn test_command_log_event() {
        let event = ActivityEvent::Command(Command::Log {
            id: 444,
            line: "Running command...".to_string(),
            is_error: false,
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
        });

        let json = serde_json::to_string(&event).unwrap();
        let parsed: ActivityEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ActivityEvent::Command(Command::Log { line, is_error, .. }) => {
                assert_eq!(line, "Running command...");
                assert!(!is_error);
            }
            _ => panic!("Expected Command::Log event"),
        }
    }

    #[test]
    fn test_operation_complete_event() {
        let event = ActivityEvent::Operation(Operation::Complete {
            id: 555,
            outcome: ActivityOutcome::Failed,
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
        });

        let json = serde_json::to_string(&event).unwrap();
        let parsed: ActivityEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ActivityEvent::Operation(Operation::Complete { outcome, .. }) => {
                assert_eq!(outcome, ActivityOutcome::Failed);
            }
            _ => panic!("Expected Operation::Complete event"),
        }
    }
}
