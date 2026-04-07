//! Builder types for creating activities.
//!
//! All activity creation goes through the [`activity!`] macro, which emits
//! `tracing::span!()` at the call site so that tracing metadata (file, line)
//! points to where the activity was created, not to this module.

use std::sync::atomic::{AtomicU64, Ordering};

use tracing::Span;

use crate::Timestamp;
use crate::activity::{Activity, ActivityType};
use crate::events::{
    ActivityEvent, ActivityLevel, Build, Command, Evaluate, Fetch, FetchKind, Operation, Process,
    Task,
};
use crate::stack::{current_activity_id, current_activity_level, send_activity_event};

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Generate a new activity ID.
/// Uses high bit to distinguish from Nix-generated IDs.
pub fn next_id() -> u64 {
    ID_COUNTER.fetch_add(1, Ordering::Relaxed) | (1 << 63)
}

/// Trait implemented by all activity builders.
///
/// Used by the [`activity!`] macro to extract name/level from a builder and
/// finalize the activity with an externally-created span.
pub trait ActivityStart: Sized {
    /// The human-readable activity name (used for `otel.name` and `devenv.user_message`).
    fn activity_name(&self) -> &str;

    /// Resolve the effective tracing level for this activity.
    fn resolved_level(&self) -> ActivityLevel;

    /// Set the pre-assigned activity ID on the builder.
    fn with_id(self, id: u64) -> Self;

    /// Finalize the activity with a span created at the call site.
    fn start_with_span(self, span: Span) -> Activity;
}

/// Create and start an activity with correct source location metadata.
///
/// This macro emits `tracing::span!()` at the call site, so tracing metadata
/// (`code.file.path`, `code.line.number`) points to where the activity is
/// created rather than to the activity library internals.
///
/// # Examples
///
/// ```ignore
/// use devenv_activity::{Activity, activity};
///
/// let act = activity!(Activity::operation("Configuring shell"));
/// let act = activity!(Activity::evaluate("Evaluating Nix"));
/// let act = activity!(Activity::build("container").derivation_path(&path));
/// let act = activity!(Activity::task("devenv:enterShell").id(42));
/// ```
#[macro_export]
macro_rules! activity {
    ($builder:expr) => {{
        let __builder = $builder;
        let __name = $crate::ActivityStart::activity_name(&__builder);
        let __otel_name = __name.to_ascii_lowercase();
        let __id = $crate::next_id();
        let __level = $crate::ActivityStart::resolved_level(&__builder);
        let __span = $crate::__create_activity_span!(__id, __otel_name.as_str(), __name, __level);
        $crate::ActivityStart::start_with_span(
            $crate::ActivityStart::with_id(__builder, __id),
            __span,
        )
    }};
}

/// Create and queue a build activity with correct source location metadata.
///
/// Same as [`activity!`] but for build activities that are waiting for a build slot.
#[macro_export]
macro_rules! start_queue {
    ($builder:expr) => {{
        let __builder: $crate::BuildBuilder = $builder;
        let __name = $crate::ActivityStart::activity_name(&__builder);
        let __otel_name = __name.to_ascii_lowercase();
        let __id = $crate::next_id();
        let __level = $crate::ActivityStart::resolved_level(&__builder);
        let __span = $crate::__create_activity_span!(__id, __otel_name.as_str(), __name, __level);
        $crate::ActivityStart::with_id(__builder, __id).queue_with_span(__span)
    }};
}

/// Internal macro that emits `span!()` at the expansion site.
///
/// Each `tracing::span!()` requires a compile-time level constant, so we
/// match on the runtime level and expand a separate `span!()` for each variant.
/// Since this macro is called from [`activity!`]/[`start_queue!`], all `span!()` calls
/// expand at the user's call site — giving correct `file!()`/`line!()`.
#[doc(hidden)]
#[macro_export]
macro_rules! __create_activity_span {
    ($id:expr, $otel_name:expr, $name:expr, $level:expr) => {
        match $level {
            $crate::ActivityLevel::Error => tracing::span!(
                tracing::Level::ERROR,
                "activity",
                activity_id = $id,
                otel.name = $otel_name,
                devenv.user_message = $name,
            ),
            $crate::ActivityLevel::Warn => tracing::span!(
                tracing::Level::WARN,
                "activity",
                activity_id = $id,
                otel.name = $otel_name,
                devenv.user_message = $name,
            ),
            $crate::ActivityLevel::Info => tracing::span!(
                tracing::Level::INFO,
                "activity",
                activity_id = $id,
                otel.name = $otel_name,
                devenv.user_message = $name,
            ),
            $crate::ActivityLevel::Debug => tracing::span!(
                tracing::Level::DEBUG,
                "activity",
                activity_id = $id,
                otel.name = $otel_name,
                devenv.user_message = $name,
            ),
            $crate::ActivityLevel::Trace => tracing::span!(
                tracing::Level::TRACE,
                "activity",
                activity_id = $id,
                otel.name = $otel_name,
                devenv.user_message = $name,
            ),
        }
    };
}

// ---------------------------------------------------------------------------
// Builder implementations
// ---------------------------------------------------------------------------

/// Builder for Build activities
pub struct BuildBuilder {
    name: String,
    derivation_path: Option<String>,
    id: Option<u64>,
    parent: Option<Option<u64>>,
    level: Option<ActivityLevel>,
}

impl BuildBuilder {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            derivation_path: None,
            id: None,
            parent: None,
            level: None,
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
        self.level = Some(level);
        self
    }

    /// Queue a build with an externally-created span (used by [`queue!`] macro).
    pub fn queue_with_span(self, span: Span) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let parent = self.parent.unwrap_or_else(current_activity_id);
        let level = self
            .level
            .or_else(current_activity_level)
            .unwrap_or_default();

        send_activity_event(ActivityEvent::Build(Build::Queued {
            id,
            name: self.name.clone(),
            parent,
            derivation_path: self.derivation_path,
            timestamp: Timestamp::now(),
        }));

        Activity::new(span, id, ActivityType::Build, level)
    }
}

impl ActivityStart for BuildBuilder {
    fn activity_name(&self) -> &str {
        &self.name
    }

    fn resolved_level(&self) -> ActivityLevel {
        self.level
            .or_else(current_activity_level)
            .unwrap_or_default()
    }

    fn with_id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    fn start_with_span(self, span: Span) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let parent = self.parent.unwrap_or_else(current_activity_id);
        let level = self.resolved_level();

        send_activity_event(ActivityEvent::Build(Build::Start {
            id,
            name: self.name.clone(),
            parent,
            derivation_path: self.derivation_path,
            timestamp: Timestamp::now(),
        }));

        Activity::new(span, id, ActivityType::Build, level)
    }
}

/// Builder for Fetch activities
pub struct FetchBuilder {
    kind: FetchKind,
    name: String,
    url: Option<String>,
    id: Option<u64>,
    parent: Option<Option<u64>>,
    level: Option<ActivityLevel>,
}

impl FetchBuilder {
    pub(crate) fn new(kind: FetchKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            url: None,
            id: None,
            parent: None,
            level: None,
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
        self.level = Some(level);
        self
    }
}

impl ActivityStart for FetchBuilder {
    fn activity_name(&self) -> &str {
        &self.name
    }

    fn resolved_level(&self) -> ActivityLevel {
        self.level
            .or_else(current_activity_level)
            .unwrap_or_default()
    }

    fn with_id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    fn start_with_span(self, span: Span) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let parent = self.parent.unwrap_or_else(current_activity_id);
        let level = self.resolved_level();

        send_activity_event(ActivityEvent::Fetch(Fetch::Start {
            id,
            kind: self.kind,
            name: self.name.clone(),
            parent,
            url: self.url,
            timestamp: Timestamp::now(),
        }));

        Activity::new(span, id, ActivityType::Fetch(self.kind), level)
    }
}

/// Builder for Evaluate activities
pub struct EvaluateBuilder {
    name: String,
    id: Option<u64>,
    parent: Option<Option<u64>>,
    level: Option<ActivityLevel>,
}

impl EvaluateBuilder {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            id: None,
            parent: None,
            level: None,
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
        self.level = Some(level);
        self
    }
}

impl ActivityStart for EvaluateBuilder {
    fn activity_name(&self) -> &str {
        &self.name
    }

    fn resolved_level(&self) -> ActivityLevel {
        self.level
            .or_else(current_activity_level)
            .unwrap_or_default()
    }

    fn with_id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    fn start_with_span(self, span: Span) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let parent = self.parent.unwrap_or_else(current_activity_id);
        let level = self.resolved_level();

        send_activity_event(ActivityEvent::Evaluate(Evaluate::Start {
            id,
            name: self.name,
            level,
            parent,
            timestamp: Timestamp::now(),
        }));

        Activity::new(span, id, ActivityType::Evaluate, level)
    }
}

/// Builder for Task activities
pub struct TaskBuilder {
    name: String,
    id: Option<u64>,
    level: Option<ActivityLevel>,
}

impl TaskBuilder {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            id: None,
            level: None,
        }
    }

    pub fn id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    pub fn level(mut self, level: ActivityLevel) -> Self {
        self.level = Some(level);
        self
    }
}

impl ActivityStart for TaskBuilder {
    fn activity_name(&self) -> &str {
        &self.name
    }

    fn resolved_level(&self) -> ActivityLevel {
        self.level
            .or_else(current_activity_level)
            .unwrap_or_default()
    }

    fn with_id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    fn start_with_span(self, span: Span) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let level = self.resolved_level();

        send_activity_event(ActivityEvent::Task(Task::Start {
            id,
            timestamp: Timestamp::now(),
        }));

        Activity::new(span, id, ActivityType::Task, level)
    }
}

/// Builder for Command activities
pub struct CommandBuilder {
    name: String,
    command: Option<String>,
    id: Option<u64>,
    parent: Option<Option<u64>>,
    level: Option<ActivityLevel>,
}

impl CommandBuilder {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: None,
            id: None,
            parent: None,
            level: None,
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
        self.level = Some(level);
        self
    }
}

impl ActivityStart for CommandBuilder {
    fn activity_name(&self) -> &str {
        &self.name
    }

    fn resolved_level(&self) -> ActivityLevel {
        self.level
            .or_else(current_activity_level)
            .unwrap_or(ActivityLevel::Debug)
    }

    fn with_id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    fn start_with_span(self, span: Span) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let parent = self.parent.unwrap_or_else(current_activity_id);
        let level = self.resolved_level();

        send_activity_event(ActivityEvent::Command(Command::Start {
            id,
            name: self.name.clone(),
            parent,
            command: self.command,
            timestamp: Timestamp::now(),
        }));

        Activity::new(span, id, ActivityType::Command, level)
    }
}

/// Builder for Process activities (long-running managed processes)
pub struct ProcessBuilder {
    name: String,
    command: Option<String>,
    ports: Vec<String>,
    ready_probe: Option<String>,
    id: Option<u64>,
    parent: Option<Option<u64>>,
    level: Option<ActivityLevel>,
}

impl ProcessBuilder {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: None,
            ports: Vec::new(),
            ready_probe: None,
            id: None,
            parent: None,
            level: None,
        }
    }

    pub fn command(mut self, cmd: impl Into<String>) -> Self {
        self.command = Some(cmd.into());
        self
    }

    pub fn ports(mut self, ports: Vec<String>) -> Self {
        self.ports = ports;
        self
    }

    pub fn ready_probe(mut self, probe: impl Into<String>) -> Self {
        self.ready_probe = Some(probe.into());
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
        self.level = Some(level);
        self
    }
}

impl ActivityStart for ProcessBuilder {
    fn activity_name(&self) -> &str {
        &self.name
    }

    fn resolved_level(&self) -> ActivityLevel {
        self.level
            .or_else(current_activity_level)
            .unwrap_or_default()
    }

    fn with_id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    fn start_with_span(self, span: Span) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let parent = self.parent.unwrap_or_else(current_activity_id);
        let level = self.resolved_level();

        send_activity_event(ActivityEvent::Process(Process::Start {
            id,
            name: self.name.clone(),
            parent,
            command: self.command,
            ports: self.ports,
            ready_probe: self.ready_probe,
            level,
            timestamp: Timestamp::now(),
        }));

        Activity::new(span, id, ActivityType::Process, level)
    }
}

/// Builder for Operation activities
pub struct OperationBuilder {
    name: String,
    detail: Option<String>,
    id: Option<u64>,
    parent: Option<Option<u64>>,
    level: Option<ActivityLevel>,
}

impl OperationBuilder {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            detail: None,
            id: None,
            parent: None,
            level: None,
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
        self.level = Some(level);
        self
    }
}

impl ActivityStart for OperationBuilder {
    fn activity_name(&self) -> &str {
        &self.name
    }

    fn resolved_level(&self) -> ActivityLevel {
        self.level
            .or_else(current_activity_level)
            .unwrap_or_default()
    }

    fn with_id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    fn start_with_span(self, span: Span) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let parent = self.parent.unwrap_or_else(current_activity_id);
        let level = self.resolved_level();

        send_activity_event(ActivityEvent::Operation(Operation::Start {
            id,
            name: self.name.clone(),
            parent,
            detail: self.detail,
            level,
            timestamp: Timestamp::now(),
        }));

        Activity::new(span, id, ActivityType::Operation, level)
    }
}
