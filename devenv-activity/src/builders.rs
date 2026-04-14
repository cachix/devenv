//! Builder types for creating activities.
//!
//! Activities can be started in two ways:
//!
//! - **Builder style**: `Activity::operation("name").start()` — convenient
//!   but span metadata (`module_path`, `file`, `line`) will point to this
//!   module since `tracing::span!()` expands here.
//!
//! - **Macro style**: `activity!(INFO, operation, "name")` or
//!   `#[instrument_activity("name")]` — creates the span at the call site
//!   so that tracing *metadata* (`module_path`, `file`, `line`) points to
//!   the caller's module. This is the preferred approach.

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
pub trait ActivityStart: Sized {
    /// The human-readable activity name (used for `otel.name` and `devenv.user_message`).
    fn activity_name(&self) -> &str;

    /// The activity kind (e.g. `"build"`, `"fetch"`, `"operation"`).
    fn activity_kind(&self) -> &'static str;

    /// Resolve the effective tracing level for this activity.
    fn resolved_level(&self) -> ActivityLevel;

    /// Return the pre-assigned activity ID, if one was set via `.id()`.
    fn existing_id(&self) -> Option<u64>;

    /// Set the pre-assigned activity ID on the builder.
    fn with_id(self, id: u64) -> Self;

    /// Finalize the activity with a span created at the call site.
    fn start_with_span(self, span: Span) -> Activity;

    /// Start the activity, creating a tracing span.
    ///
    /// Prefer the [`start!`] macro which expands the span at the call site,
    /// giving correct `code.file.path` / `code.module.name` metadata.
    /// This method exists for use inside other macros that already handle
    /// span creation (e.g. `activity!`, `#[instrument_activity]`).
    fn start(self) -> Activity {
        let id = self.existing_id().unwrap_or_else(crate::next_id);
        let span = crate::__create_activity_span!(&self, id);
        self.with_id(id).start_with_span(span)
    }
}

/// Start an activity from any builder expression.
///
/// Expands the tracing span at the call site so that span metadata
/// (`code.file.path`, `code.line.number`, `code.module.name`) points
/// to the caller's module.
///
/// ```ignore
/// // Simple — shorthand for Activity::operation(...).start()
/// start!(Activity::operation("Running MCP server").detail(format!(...)))
/// start!(Activity::fetch(FetchKind::Download, name).url(&u).id(id))
/// start!(Activity::task(&name).id(activity_id))
/// ```
#[macro_export]
macro_rules! start {
    ($builder:expr) => {{
        let __builder = $builder;
        let __id = $crate::ActivityStart::existing_id(&__builder).unwrap_or_else($crate::next_id);
        let __span = $crate::__create_activity_span!(&__builder, __id);
        $crate::ActivityStart::start_with_span(
            $crate::ActivityStart::with_id(__builder, __id),
            __span,
        )
    }};
}

/// Queue a build activity, expanding the span at the call site.
///
/// ```ignore
/// queue!(Activity::build("foo").derivation_path(drv).parent(parent_id))
/// ```
#[macro_export]
macro_rules! queue {
    ($builder:expr) => {{
        let __builder = $builder;
        let __id = $crate::ActivityStart::existing_id(&__builder).unwrap_or_else($crate::next_id);
        let __span = $crate::__create_activity_span!(&__builder, __id);
        $crate::ActivityStart::with_id(__builder, __id).queue_with_span(__span)
    }};
}

/// Create and start an activity (shorthand).
///
/// ```ignore
/// activity!(INFO, operation, "Configuring shell")
/// activity!(DEBUG, evaluate, format!("Checking cachix.{}", field))
/// ```
#[macro_export]
macro_rules! activity {
    ($level:ident, $kind:ident, $name:expr) => {
        $crate::start!(
            $crate::__activity_builder!($kind, $name).level($crate::__to_activity_level!($level))
        )
    };
}

/// Map a kind keyword to a builder constructor.
#[doc(hidden)]
#[macro_export]
macro_rules! __activity_builder {
    (build, $name:expr) => {
        $crate::Activity::build($name)
    };
    (operation, $name:expr) => {
        $crate::Activity::operation($name)
    };
    (evaluate, $name:expr) => {
        $crate::Activity::evaluate($name)
    };
    (task, $name:expr) => {
        $crate::Activity::task($name)
    };
    (command, $name:expr) => {
        $crate::Activity::command($name)
    };
    (process, $name:expr) => {
        $crate::Activity::process($name)
    };
}

/// Map a level keyword to `ActivityLevel`.
#[doc(hidden)]
#[macro_export]
macro_rules! __to_activity_level {
    (ERROR) => {
        $crate::ActivityLevel::Error
    };
    (WARN) => {
        $crate::ActivityLevel::Warn
    };
    (INFO) => {
        $crate::ActivityLevel::Info
    };
    (DEBUG) => {
        $crate::ActivityLevel::Debug
    };
    (TRACE) => {
        $crate::ActivityLevel::Trace
    };
}

/// Create an activity span using tracing's lower-level API directly.
///
/// Used by [`ActivityStart::start()`] and [`BuildBuilder::queue()`] to create
/// tracing spans with the base activity fields plus any extra fields.
#[doc(hidden)]
#[macro_export]
macro_rules! __create_activity_span {
    ($builder:expr, $id:expr $(, $($($k:ident).+ = $v:expr),+ )?) => {{
        use tracing::__macro_support::Callsite as _;
        use tracing::callsite::{DefaultCallsite, Identifier};
        use tracing::field::FieldSet;

        // Common fields + caller-supplied extras. Each call site gets its own set.
        const FIELD_NAMES: &[&str] = &[
            "activity_id",
            "otel.name",
            "devenv.user_message",
            "devenv.activity.kind",
            "devenv.outcome",
            "otel.status_code",
            $($( stringify!($($k).+) ),+ )?
        ];

        macro_rules! def_callsite {
            ($lvl:expr, $CS:ident, $META:ident) => {
                static $META: tracing::Metadata<'static> = tracing::Metadata::new(
                    "activity",
                    module_path!(),
                    $lvl,
                    Some(file!()),
                    Some(line!()),
                    Some(module_path!()),
                    FieldSet::new(FIELD_NAMES, Identifier(&$CS)),
                    tracing::metadata::Kind::SPAN,
                );
                static $CS: DefaultCallsite = DefaultCallsite::new(&$META);
            };
        }

        def_callsite!(tracing::Level::ERROR, CS_E, M_E);
        def_callsite!(tracing::Level::WARN, CS_W, M_W);
        def_callsite!(tracing::Level::INFO, CS_I, M_I);
        def_callsite!(tracing::Level::DEBUG, CS_D, M_D);
        def_callsite!(tracing::Level::TRACE, CS_T, M_T);

        let __level = $crate::ActivityStart::resolved_level($builder);
        let cs: &DefaultCallsite = match __level {
            $crate::ActivityLevel::Error => &CS_E,
            $crate::ActivityLevel::Warn => &CS_W,
            $crate::ActivityLevel::Info => &CS_I,
            $crate::ActivityLevel::Debug => &CS_D,
            $crate::ActivityLevel::Trace => &CS_T,
        };

        let interest = cs.interest();
        if interest.is_never() {
            tracing::Span::none()
        } else {
            let meta = cs.metadata();
            if tracing::__macro_support::__is_enabled(meta, interest) {
                let __name = $crate::ActivityStart::activity_name($builder);
                let __kind = $crate::ActivityStart::activity_kind($builder);
                let __otel_name_owned;
                let __otel_name: &str = if __name.as_bytes().iter().any(|b| b.is_ascii_uppercase()) {
                    __otel_name_owned = __name.to_ascii_lowercase();
                    __otel_name_owned.as_str()
                } else {
                    __name
                };
                let fs = meta.fields();
                tracing::Span::new(
                    meta,
                    &fs.value_set(&[
                        (&fs.field("activity_id").unwrap(), Some(&$id as &dyn tracing::field::Value)),
                        (&fs.field("otel.name").unwrap(), Some(&__otel_name as &dyn tracing::field::Value)),
                        (&fs.field("devenv.user_message").unwrap(), Some(&__name as &dyn tracing::field::Value)),
                        (&fs.field("devenv.activity.kind").unwrap(), Some(&__kind as &dyn tracing::field::Value)),
                        $($( (&fs.field(stringify!($($k).+)).unwrap(), Some(&$v as &dyn tracing::field::Value)) ),+ )?
                    ]),
                )
            } else {
                tracing::Span::none()
            }
        }
    }};
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

    /// Queue a build activity.
    ///
    /// Prefer the [`queue!`] macro for correct call-site metadata.
    pub fn queue(self) -> Activity {
        let id = self.existing_id().unwrap_or_else(next_id);
        let span = crate::__create_activity_span!(&self, id);
        self.with_id(id).queue_with_span(span)
    }

    /// Queue a build with an externally-created span.
    pub fn queue_with_span(self, span: Span) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let parent = self.parent.unwrap_or_else(current_activity_id);
        let level = self.resolved_level();

        send_activity_event(ActivityEvent::Build(Build::Queued {
            id,
            name: self.name,
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

    fn activity_kind(&self) -> &'static str {
        "build"
    }

    fn resolved_level(&self) -> ActivityLevel {
        self.level
            .or_else(current_activity_level)
            .unwrap_or_default()
    }

    fn existing_id(&self) -> Option<u64> {
        self.id
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
            name: self.name,
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

    fn activity_kind(&self) -> &'static str {
        "fetch"
    }

    fn resolved_level(&self) -> ActivityLevel {
        self.level
            .or_else(current_activity_level)
            .unwrap_or_default()
    }

    fn existing_id(&self) -> Option<u64> {
        self.id
    }

    fn with_id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }

    fn start_with_span(self, span: Span) -> Activity {
        let id = self.id.unwrap_or_else(next_id);
        let parent = self.parent.unwrap_or_else(current_activity_id);
        let level = self.resolved_level();
        let kind = self.kind;

        send_activity_event(ActivityEvent::Fetch(Fetch::Start {
            id,
            kind,
            name: self.name,
            parent,
            url: self.url,
            timestamp: Timestamp::now(),
        }));

        Activity::new(span, id, ActivityType::Fetch(kind), level)
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

    fn activity_kind(&self) -> &'static str {
        "evaluate"
    }

    fn resolved_level(&self) -> ActivityLevel {
        self.level
            .or_else(current_activity_level)
            .unwrap_or_default()
    }

    fn existing_id(&self) -> Option<u64> {
        self.id
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

    fn activity_kind(&self) -> &'static str {
        "task"
    }

    fn resolved_level(&self) -> ActivityLevel {
        self.level
            .or_else(current_activity_level)
            .unwrap_or_default()
    }

    fn existing_id(&self) -> Option<u64> {
        self.id
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

    fn activity_kind(&self) -> &'static str {
        "command"
    }

    fn resolved_level(&self) -> ActivityLevel {
        self.level
            .or_else(current_activity_level)
            .unwrap_or(ActivityLevel::Debug)
    }

    fn existing_id(&self) -> Option<u64> {
        self.id
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
            name: self.name,
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

    fn activity_kind(&self) -> &'static str {
        "process"
    }

    fn resolved_level(&self) -> ActivityLevel {
        self.level
            .or_else(current_activity_level)
            .unwrap_or_default()
    }

    fn existing_id(&self) -> Option<u64> {
        self.id
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
            name: self.name,
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

    fn activity_kind(&self) -> &'static str {
        "operation"
    }

    fn resolved_level(&self) -> ActivityLevel {
        self.level
            .or_else(current_activity_level)
            .unwrap_or_default()
    }

    fn existing_id(&self) -> Option<u64> {
        self.id
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
            name: self.name,
            parent,
            detail: self.detail,
            level,
            timestamp: Timestamp::now(),
        }));

        Activity::new(span, id, ActivityType::Operation, level)
    }
}
