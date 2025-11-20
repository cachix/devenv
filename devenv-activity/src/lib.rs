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
//! Activities automatically deref to `Span`, giving you access to all span methods:
//!
//! ```ignore
//! use tracing::Instrument;
//! use devenv_activity::Activity;
//!
//! // Create an activity
//! let activity = Activity::build("my-task");
//!
//! // Use span methods via Deref
//! activity.in_scope(|| {
//!     // Code runs with activity context
//! });
//!
//! // Instrument async code
//! async_fn().instrument(activity.span()).await;
//!
//! // Enter/exit manually
//! let _guard = activity.enter();
//! ```

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::ops::Deref;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::time::SystemTime;
use tokio::sync::mpsc;
use tracing::{span, Event, Level, Span, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

// Re-export for convenience
pub use tracing_subscriber::Registry;

// ---------------------------------------------------------------------------
// Timestamp Serialization
// ---------------------------------------------------------------------------

mod timestamp_format {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn serialize<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let duration = time
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0));
        let secs = duration.as_secs();
        let nanos = duration.subsec_nanos();

        let total_secs = secs as i64;
        let total_nanos = nanos;

        let dt_secs = total_secs;
        let dt_nanos = total_nanos as i64;

        let year = 1970 + (dt_secs / 31_557_600);
        let remaining = dt_secs % 31_557_600;
        let days = remaining / 86400;
        let day_secs = remaining % 86400;
        let hours = day_secs / 3600;
        let minutes = (day_secs % 3600) / 60;
        let seconds = day_secs % 60;

        let (month, day) = days_to_month_day(days as u32, is_leap_year(year));

        let timestamp = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}Z",
            year, month, day, hours, minutes, seconds, dt_nanos
        );

        serializer.serialize_str(&timestamp)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        parse_rfc3339(&s).map_err(serde::de::Error::custom)
    }

    fn is_leap_year(year: i64) -> bool {
        (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
    }

    fn days_to_month_day(mut days: u32, leap: bool) -> (u32, u32) {
        let days_in_month = if leap {
            [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        } else {
            [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        };

        for (i, &dim) in days_in_month.iter().enumerate() {
            if days < dim {
                return ((i + 1) as u32, days + 1);
            }
            days -= dim;
        }
        (12, 31)
    }

    fn parse_rfc3339(s: &str) -> Result<SystemTime, String> {
        let parts: Vec<&str> = s.split('T').collect();
        if parts.len() != 2 {
            return Err("Invalid RFC3339 format".to_string());
        }

        let date_parts: Vec<&str> = parts[0].split('-').collect();
        if date_parts.len() != 3 {
            return Err("Invalid date format".to_string());
        }

        let year: i32 = date_parts[0]
            .parse()
            .map_err(|_| "Invalid year".to_string())?;
        let month: u32 = date_parts[1]
            .parse()
            .map_err(|_| "Invalid month".to_string())?;
        let day: u32 = date_parts[2]
            .parse()
            .map_err(|_| "Invalid day".to_string())?;

        let time_and_tz = parts[1].trim_end_matches('Z');
        let time_parts: Vec<&str> = time_and_tz.split(':').collect();
        if time_parts.len() < 2 {
            return Err("Invalid time format".to_string());
        }

        let hour: u32 = time_parts[0]
            .parse()
            .map_err(|_| "Invalid hour".to_string())?;
        let minute: u32 = time_parts[1]
            .parse()
            .map_err(|_| "Invalid minute".to_string())?;

        let (second, _nanos) = if time_parts.len() > 2 {
            let sec_parts: Vec<&str> = time_parts[2].split('.').collect();
            let sec: u32 = sec_parts[0]
                .parse()
                .map_err(|_| "Invalid second".to_string())?;
            let nanos = if sec_parts.len() > 1 {
                let nanos_str = format!("{:0<9}", sec_parts[1]);
                nanos_str[..9]
                    .parse()
                    .map_err(|_| "Invalid nanoseconds".to_string())?
            } else {
                0
            };
            (sec, nanos)
        } else {
            (0, 0)
        };

        let days_since_epoch = days_from_civil(year, month, day);
        let secs = days_since_epoch as u64 * 86400 + hour as u64 * 3600 + minute as u64 * 60 + second as u64;

        Ok(UNIX_EPOCH + Duration::from_secs(secs))
    }

    fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
        let y = year as i64 - if month <= 2 { 1 } else { 0 };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = (y - era * 400) as u32;
        let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe as i64 - 719468
    }
}

// Thread-local for passing ActivityData to the layer during span creation
thread_local! {
    static ACTIVITY_DATA: RefCell<Option<ActivityData>> = const { RefCell::new(None) };
}

// Thread-local stack for tracking current Activity IDs (for parent detection)
thread_local! {
    static ACTIVITY_STACK: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

// ---------------------------------------------------------------------------
// Core Types
// ---------------------------------------------------------------------------

/// All activity events - single source of truth
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ActivityEvent {
    /// Activity started
    Start {
        id: u64,
        kind: ActivityKind,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        #[serde(with = "timestamp_format")]
        timestamp: SystemTime,
    },

    /// Activity completed
    Complete {
        id: u64,
        outcome: ActivityOutcome,
        #[serde(with = "timestamp_format")]
        timestamp: SystemTime,
    },

    /// Progress update
    Progress {
        id: u64,
        progress: ProgressState,
        #[serde(with = "timestamp_format")]
        timestamp: SystemTime,
    },

    /// Phase/step change
    Phase {
        id: u64,
        phase: String,
        #[serde(with = "timestamp_format")]
        timestamp: SystemTime,
    },

    /// Log line from activity
    Log {
        id: u64,
        line: String,
        #[serde(default)]
        is_error: bool,
        #[serde(with = "timestamp_format")]
        timestamp: SystemTime,
    },

    /// Message not tied to an activity
    Message {
        level: LogLevel,
        text: String,
        #[serde(with = "timestamp_format")]
        timestamp: SystemTime,
    },
}

/// Explicit progress representation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ProgressState {
    /// Known total amount
    Determinate {
        current: u64,
        total: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        unit: Option<ProgressUnit>,
    },
    /// Unknown total, just tracking work done
    Indeterminate {
        current: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        unit: Option<ProgressUnit>,
    },
}

/// Unit of progress measurement
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProgressUnit {
    Bytes,
    Files,
    Items,
}

/// Generic kinds that map to any build tool
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActivityKind {
    /// Building/compiling (Nix builds, compilation)
    Build,
    /// Fetching/downloading (store paths, dependencies)
    Fetch,
    /// Evaluating expressions (Nix eval, config parsing)
    Evaluate,
    /// User-defined task (devenv tasks)
    Task,
    /// Shell command execution
    Command,
    /// Generic operation
    Operation,
}

impl std::fmt::Display for ActivityKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActivityKind::Build => write!(f, "build"),
            ActivityKind::Fetch => write!(f, "fetch"),
            ActivityKind::Evaluate => write!(f, "evaluate"),
            ActivityKind::Task => write!(f, "task"),
            ActivityKind::Command => write!(f, "command"),
            ActivityKind::Operation => write!(f, "operation"),
        }
    }
}

impl std::str::FromStr for ActivityKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "build" => Ok(ActivityKind::Build),
            "fetch" => Ok(ActivityKind::Fetch),
            "evaluate" => Ok(ActivityKind::Evaluate),
            "task" => Ok(ActivityKind::Task),
            "command" => Ok(ActivityKind::Command),
            "operation" => Ok(ActivityKind::Operation),
            _ => Err(()),
        }
    }
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

/// Log level for standalone messages
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

// ---------------------------------------------------------------------------
// Span Extension Data
// ---------------------------------------------------------------------------

/// Data attached to activity spans via extensions
pub struct ActivityData {
    pub id: u64,
    pub kind: ActivityKind,
    pub name: String,
    pub parent: Option<u64>,
    pub detail: Option<String>,
    outcome: AtomicU8,
}

impl ActivityData {
    /// Create new ActivityData with Success outcome
    pub fn new(
        id: u64,
        kind: ActivityKind,
        name: String,
        parent: Option<u64>,
        detail: Option<String>,
    ) -> Self {
        Self {
            id,
            kind,
            name,
            parent,
            detail,
            outcome: AtomicU8::new(ActivityOutcome::Success.to_u8()),
        }
    }

    /// Get the outcome
    pub fn outcome(&self) -> ActivityOutcome {
        ActivityOutcome::from_u8(self.outcome.load(Ordering::Relaxed))
    }

    /// Set the outcome
    pub fn set_outcome(&self, outcome: ActivityOutcome) {
        self.outcome.store(outcome.to_u8(), Ordering::Relaxed);
    }
}

impl ActivityOutcome {
    fn to_u8(self) -> u8 {
        match self {
            ActivityOutcome::Success => 0,
            ActivityOutcome::Failed => 1,
            ActivityOutcome::Cancelled => 2,
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            0 => ActivityOutcome::Success,
            1 => ActivityOutcome::Failed,
            2 => ActivityOutcome::Cancelled,
            _ => ActivityOutcome::Success,
        }
    }
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
// Activity Guard
// ---------------------------------------------------------------------------

/// Guard that tracks an activity's lifecycle via tracing spans.
/// Activity is Send + Sync, allowing storage in Mutex for async callbacks.
#[must_use = "Activity will complete immediately if dropped"]
pub struct Activity {
    span: Span,
    id: u64,
}

impl Activity {
    /// Start a build activity
    pub fn build(name: impl Into<String>) -> Self {
        Self::start(ActivityKind::Build, name.into(), None)
    }

    /// Start a build activity with detail
    pub fn build_with_detail(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::start(ActivityKind::Build, name.into(), Some(detail.into()))
    }

    /// Start a fetch activity
    pub fn fetch(name: impl Into<String>) -> Self {
        Self::start(ActivityKind::Fetch, name.into(), None)
    }

    /// Start an evaluate activity
    pub fn evaluate(name: impl Into<String>) -> Self {
        Self::start(ActivityKind::Evaluate, name.into(), None)
    }

    /// Start a task activity
    pub fn task(name: impl Into<String>) -> Self {
        Self::start(ActivityKind::Task, name.into(), None)
    }

    /// Start a command activity
    pub fn command(name: impl Into<String>, cmd: impl Into<String>) -> Self {
        Self::start(ActivityKind::Command, name.into(), Some(cmd.into()))
    }

    /// Start a generic operation
    pub fn operation(name: impl Into<String>) -> Self {
        Self::start(ActivityKind::Operation, name.into(), None)
    }

    /// Start an activity with a specific external ID (for Nix integration)
    pub fn start_with_id(
        id: u64,
        kind: ActivityKind,
        name: String,
        parent: Option<u64>,
        detail: Option<String>,
    ) -> Self {
        Self::start_internal(id, kind, name, parent, detail)
    }

    /// Start an activity with explicit kind
    fn start(kind: ActivityKind, name: String, detail: Option<String>) -> Self {
        let id = next_id();

        // Get parent from current span context
        let parent = get_current_activity_id();

        Self::start_internal(id, kind, name, parent, detail)
    }

    fn start_internal(
        id: u64,
        kind: ActivityKind,
        name: String,
        parent: Option<u64>,
        detail: Option<String>,
    ) -> Self {
        // Store data in thread-local for the layer to retrieve and add to extensions
        ACTIVITY_DATA.with(|p| {
            *p.borrow_mut() = Some(ActivityData::new(id, kind, name, parent, detail));
        });

        // Create the tracing span - on_new_span fires synchronously and retrieves the data
        // Only include activity_id as a field for basic tracing visibility
        let span = span!(
            Level::TRACE,
            "activity",
            activity_id = id,
        );

        // Push to activity stack for parent tracking
        ACTIVITY_STACK.with(|stack| stack.borrow_mut().push(id));

        Self { span, id }
    }

    /// Get the activity ID
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Get a cloned span for this activity.
    ///
    /// Useful when you need to pass a span by value, such as to `.instrument()`.
    pub fn span(&self) -> Span {
        self.span.clone()
    }

    /// Mark as failed
    pub fn fail(&self) {
        self.set_outcome(ActivityOutcome::Failed);
    }

    /// Mark as cancelled
    pub fn cancel(&self) {
        self.set_outcome(ActivityOutcome::Cancelled);
    }

    fn set_outcome(&self, outcome: ActivityOutcome) {
        if let Some(span_id) = self.span.id() {
            tracing::dispatcher::get_default(|dispatch| {
                if let Some(subscriber) = dispatch.downcast_ref::<tracing_subscriber::Registry>() {
                    if let Some(span_ref) = subscriber.span(&span_id) {
                        if let Some(data) = span_ref.extensions().get::<ActivityData>() {
                            data.set_outcome(outcome);
                        }
                    }
                }
            });
        }
    }

    /// Update progress (determinate)
    pub fn progress(&self, current: u64, total: u64) {
        let _guard = self.span.enter();
        tracing::trace!(
            target: "devenv::activity",
            activity_id = self.id,
            progress_kind = "determinate",
            progress_current = current,
            progress_total = total,
        );
    }

    /// Update progress with byte unit
    pub fn progress_bytes(&self, current: u64, total: u64) {
        let _guard = self.span.enter();
        tracing::trace!(
            target: "devenv::activity",
            activity_id = self.id,
            progress_kind = "determinate",
            progress_current = current,
            progress_total = total,
            progress_unit = "bytes",
        );
    }

    /// Update progress (indeterminate)
    pub fn progress_indeterminate(&self, current: u64) {
        let _guard = self.span.enter();
        tracing::trace!(
            target: "devenv::activity",
            activity_id = self.id,
            progress_kind = "indeterminate",
            progress_current = current,
        );
    }

    /// Update phase
    pub fn phase(&self, phase: impl Into<String>) {
        let _guard = self.span.enter();
        let phase_str = phase.into();
        tracing::trace!(
            target: "devenv::activity",
            activity_id = self.id,
            phase = %phase_str,
        );
    }

    /// Log a line
    pub fn log(&self, line: impl Into<String>) {
        let _guard = self.span.enter();
        let line_str = line.into();
        tracing::trace!(
            target: "devenv::activity",
            activity_id = self.id,
            log_line = %line_str,
            log_is_error = false,
        );
    }

    /// Log an error
    pub fn error(&self, line: impl Into<String>) {
        let _guard = self.span.enter();
        let line_str = line.into();
        tracing::trace!(
            target: "devenv::activity",
            activity_id = self.id,
            log_line = %line_str,
            log_is_error = true,
        );
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
        Self {
            span: self.span.clone(),
            id: self.id,
        }
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        // Pop from activity stack
        ACTIVITY_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            if stack.last() == Some(&self.id) {
                stack.pop();
            }
        });
    }
}

/// Get the activity ID from the current activity stack
fn get_current_activity_id() -> Option<u64> {
    ACTIVITY_STACK.with(|stack| stack.borrow().last().copied())
}

// ---------------------------------------------------------------------------
// Activity Layer (Core tracing integration)
// ---------------------------------------------------------------------------

/// Layer that stores ActivityData in span extensions.
/// This is the core layer that makes the activity system work with tracing.
pub struct ActivityLayer;

impl ActivityLayer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ActivityLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for ActivityLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        // Only process activity spans
        if attrs.metadata().name() != "activity" {
            return;
        }

        let Some(span) = ctx.span(id) else { return };

        // Retrieve ActivityData from thread-local
        let data = ACTIVITY_DATA.with(|p| p.borrow_mut().take());
        let Some(data) = data else { return };

        // Store in span extensions
        span.extensions_mut().insert(data);
    }
}

// ---------------------------------------------------------------------------
// Activity Event Forwarder (TUI consumer)
// ---------------------------------------------------------------------------

/// Layer that forwards ActivityEvents to a channel for TUI consumption.
/// Must be registered after ActivityLayer so extensions are populated.
pub struct ActivityEventForwarder {
    tx: mpsc::UnboundedSender<ActivityEvent>,
}

impl ActivityEventForwarder {
    pub fn new(tx: mpsc::UnboundedSender<ActivityEvent>) -> Self {
        Self { tx }
    }
}

impl<S> Layer<S> for ActivityEventForwarder
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        // Only process activity spans
        if attrs.metadata().name() != "activity" {
            return;
        }

        let Some(span) = ctx.span(id) else { return };
        let extensions = span.extensions();
        let Some(data) = extensions.get::<ActivityData>() else {
            return;
        };

        // Send Start event
        let _ = self.tx.send(ActivityEvent::Start {
            id: data.id,
            kind: data.kind.clone(),
            name: data.name.clone(),
            parent: data.parent,
            detail: data.detail.clone(),
            timestamp: SystemTime::now(),
        });
    }

    fn on_close(&self, id: span::Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };

        // Only process activity spans
        if span.metadata().name() != "activity" {
            return;
        }

        let extensions = span.extensions();
        let Some(data) = extensions.get::<ActivityData>() else {
            return;
        };

        let _ = self.tx.send(ActivityEvent::Complete {
            id: data.id,
            outcome: data.outcome(),
            timestamp: SystemTime::now(),
        });
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        // Only process our activity events
        if event.metadata().target() != "devenv::activity" {
            return;
        }

        let mut visitor = ActivityEventVisitor::default();
        event.record(&mut visitor);

        if let Some(evt) = visitor.into_event() {
            let _ = self.tx.send(evt);
        }
    }
}

/// Visitor that extracts activity event fields
#[derive(Default)]
struct ActivityEventVisitor {
    activity_id: Option<u64>,
    // Progress fields
    progress_kind: Option<String>,
    progress_current: Option<u64>,
    progress_total: Option<u64>,
    progress_unit: Option<String>,
    // Phase field
    phase: Option<String>,
    // Log fields
    log_line: Option<String>,
    log_is_error: bool,
    // Message fields
    message_level: Option<String>,
    message_text: Option<String>,
}

impl tracing::field::Visit for ActivityEventVisitor {
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        match field.name() {
            "activity_id" => self.activity_id = Some(value),
            "progress_current" => self.progress_current = Some(value),
            "progress_total" => self.progress_total = Some(value),
            _ => {}
        }
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        if field.name() == "log_is_error" {
            self.log_is_error = value;
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "progress_kind" => self.progress_kind = Some(value.to_string()),
            "progress_unit" => self.progress_unit = Some(value.to_string()),
            "phase" => self.phase = Some(value.to_string()),
            "log_line" => self.log_line = Some(value.to_string()),
            "message_level" => self.message_level = Some(value.to_string()),
            "message_text" => self.message_text = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        // Handle Display fields that come through as debug
        let s = format!("{:?}", value);
        self.record_str(field, s.trim_matches('"'));
    }
}

impl ActivityEventVisitor {
    fn into_event(self) -> Option<ActivityEvent> {
        let timestamp = SystemTime::now();

        // Check for standalone message first (no activity_id)
        if let (Some(level_str), Some(text)) = (&self.message_level, self.message_text) {
            let level = match level_str.as_str() {
                "error" | "Error" => LogLevel::Error,
                "warn" | "Warn" => LogLevel::Warn,
                "info" | "Info" => LogLevel::Info,
                "debug" | "Debug" => LogLevel::Debug,
                _ => LogLevel::Trace,
            };
            return Some(ActivityEvent::Message {
                level,
                text,
                timestamp,
            });
        }

        let id = self.activity_id?;

        // Check what kind of event this is based on fields present
        if let Some(phase) = self.phase {
            return Some(ActivityEvent::Phase {
                id,
                phase,
                timestamp,
            });
        }

        if let Some(line) = self.log_line {
            return Some(ActivityEvent::Log {
                id,
                line,
                is_error: self.log_is_error,
                timestamp,
            });
        }

        if let Some(current) = self.progress_current {
            let unit = self.progress_unit.and_then(|u| match u.as_str() {
                "bytes" => Some(ProgressUnit::Bytes),
                "files" => Some(ProgressUnit::Files),
                "items" => Some(ProgressUnit::Items),
                _ => None,
            });

            let progress = match (self.progress_kind.as_deref(), self.progress_total) {
                (Some("determinate"), Some(total)) => ProgressState::Determinate {
                    current,
                    total,
                    unit,
                },
                _ => ProgressState::Indeterminate { current, unit },
            };

            return Some(ActivityEvent::Progress {
                id,
                progress,
                timestamp,
            });
        }

        None
    }
}

// ---------------------------------------------------------------------------
// Standalone Message API
// ---------------------------------------------------------------------------

/// Emit a standalone message not tied to an activity
pub fn message(level: LogLevel, text: impl Into<String>) {
    let level_str = match level {
        LogLevel::Error => "error",
        LogLevel::Warn => "warn",
        LogLevel::Info => "info",
        LogLevel::Debug => "debug",
        LogLevel::Trace => "trace",
    };
    let text_str = text.into();
    tracing::trace!(
        target: "devenv::activity",
        message_level = level_str,
        message_text = %text_str,
    );
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Handle for creating activity tracing layers
pub struct ActivityHandle {
    tx: mpsc::UnboundedSender<ActivityEvent>,
}

impl ActivityHandle {
    /// Create the core activity layer that stores data in span extensions
    pub fn activity_layer(&self) -> ActivityLayer {
        ActivityLayer::new()
    }

    /// Create the event forwarder layer that sends events to the receiver
    pub fn forwarder_layer(&self) -> ActivityEventForwarder {
        ActivityEventForwarder::new(self.tx.clone())
    }
}

/// Initialize the activity system.
/// Returns receiver for TUI and a handle for creating layers.
///
/// Usage:
/// ```rust,ignore
/// let (rx, activity) = devenv_activity::init();
/// Registry::default()
///     .with(activity.activity_layer())
///     .with(activity.forwarder_layer())
/// ```
pub fn init() -> (mpsc::UnboundedReceiver<ActivityEvent>, ActivityHandle) {
    let (tx, rx) = mpsc::unbounded_channel();
    (rx, ActivityHandle { tx })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    /// Helper to set up the subscriber with both layers
    fn setup_test() -> (mpsc::UnboundedReceiver<ActivityEvent>, tracing::subscriber::DefaultGuard) {
        let (tx, rx) = mpsc::unbounded_channel();
        let activity_layer = ActivityLayer::new();
        let forwarder = ActivityEventForwarder::new(tx);

        let subscriber = tracing_subscriber::registry()
            .with(activity_layer)
            .with(forwarder);
        let guard = tracing::subscriber::set_default(subscriber);
        (rx, guard)
    }

    #[tokio::test]
    async fn test_activity_lifecycle() {
        let (mut rx, _guard) = setup_test();

        // Create and drop an activity
        {
            let _activity = Activity::build("test-package");
        }

        // Verify events
        let start = rx.recv().await.unwrap();
        match start {
            ActivityEvent::Start { name, kind, .. } => {
                assert_eq!(name, "test-package");
                assert_eq!(kind, ActivityKind::Build);
            }
            _ => panic!("Expected Start event"),
        }

        let complete = rx.recv().await.unwrap();
        match complete {
            ActivityEvent::Complete { outcome, .. } => {
                assert_eq!(outcome, ActivityOutcome::Success);
            }
            _ => panic!("Expected Complete event"),
        }
    }

    #[tokio::test]
    async fn test_activity_failure() {
        let (mut rx, _guard) = setup_test();

        {
            let activity = Activity::build("failing-package");
            activity.fail();
        }

        // Skip Start event
        let _ = rx.recv().await.unwrap();

        let complete = rx.recv().await.unwrap();
        match complete {
            ActivityEvent::Complete { outcome, .. } => {
                assert_eq!(outcome, ActivityOutcome::Failed);
            }
            _ => panic!("Expected Complete event"),
        }
    }

    #[tokio::test]
    async fn test_progress_events() {
        let (mut rx, _guard) = setup_test();

        {
            let activity = Activity::build("package");
            activity.progress(50, 100);
        }

        // Skip Start event
        let _ = rx.recv().await.unwrap();

        // Get Progress event
        let progress = rx.recv().await.unwrap();
        match progress {
            ActivityEvent::Progress { progress, .. } => match progress {
                ProgressState::Determinate { current, total, .. } => {
                    assert_eq!(current, 50);
                    assert_eq!(total, 100);
                }
                _ => panic!("Expected Determinate progress"),
            },
            _ => panic!("Expected Progress event"),
        }
    }

    #[tokio::test]
    async fn test_phase_events() {
        let (mut rx, _guard) = setup_test();

        {
            let activity = Activity::build("package");
            activity.phase("configure");
        }

        // Skip Start event
        let _ = rx.recv().await.unwrap();

        // Get Phase event
        let phase = rx.recv().await.unwrap();
        match phase {
            ActivityEvent::Phase { phase, .. } => {
                assert_eq!(phase, "configure");
            }
            _ => panic!("Expected Phase event"),
        }
    }

    #[tokio::test]
    async fn test_log_events() {
        let (mut rx, _guard) = setup_test();

        {
            let activity = Activity::build("package");
            activity.log("Building...");
            activity.error("Error occurred");
        }

        // Skip Start event
        let _ = rx.recv().await.unwrap();

        // Get Log events
        let log1 = rx.recv().await.unwrap();
        match log1 {
            ActivityEvent::Log {
                line, is_error, ..
            } => {
                assert_eq!(line, "Building...");
                assert!(!is_error);
            }
            _ => panic!("Expected Log event"),
        }

        let log2 = rx.recv().await.unwrap();
        match log2 {
            ActivityEvent::Log {
                line, is_error, ..
            } => {
                assert_eq!(line, "Error occurred");
                assert!(is_error);
            }
            _ => panic!("Expected Log event"),
        }
    }

    #[tokio::test]
    async fn test_nested_activities() {
        let (mut rx, _guard) = setup_test();

        let parent_id;
        {
            let parent = Activity::build("parent");
            parent_id = parent.id();
            {
                let _child = Activity::fetch("child");
            }
        }

        // Get parent start
        let parent_start = rx.recv().await.unwrap();
        match parent_start {
            ActivityEvent::Start { id, parent, .. } => {
                assert_eq!(id, parent_id);
                assert!(parent.is_none());
            }
            _ => panic!("Expected Start event"),
        }

        // Get child start - should have parent ID set
        let child_start = rx.recv().await.unwrap();
        match child_start {
            ActivityEvent::Start { parent, .. } => {
                assert_eq!(parent, Some(parent_id));
            }
            _ => panic!("Expected Start event"),
        }
    }

    #[tokio::test]
    async fn test_standalone_message() {
        let (mut rx, _guard) = setup_test();

        message(LogLevel::Warn, "Cache miss for foo");

        let msg = rx.recv().await.unwrap();
        match msg {
            ActivityEvent::Message { level, text, .. } => {
                assert_eq!(level, LogLevel::Warn);
                assert_eq!(text, "Cache miss for foo");
            }
            _ => panic!("Expected Message event"),
        }
    }

    #[test]
    fn test_activity_kind_display_parse() {
        let kinds = [
            ActivityKind::Build,
            ActivityKind::Fetch,
            ActivityKind::Evaluate,
            ActivityKind::Task,
            ActivityKind::Command,
            ActivityKind::Operation,
        ];

        for kind in kinds {
            let s = kind.to_string();
            let parsed: ActivityKind = s.parse().unwrap();
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn test_activity_event_serialization() {
        let event = ActivityEvent::Start {
            id: 123,
            kind: ActivityKind::Build,
            name: "test".to_string(),
            parent: None,
            detail: Some("detail".to_string()),
            timestamp: SystemTime::UNIX_EPOCH,
        };

        let json = serde_json::to_string(&event).unwrap();
        let parsed: ActivityEvent = serde_json::from_str(&json).unwrap();

        match parsed {
            ActivityEvent::Start { id, name, .. } => {
                assert_eq!(id, 123);
                assert_eq!(name, "test");
            }
            _ => panic!("Expected Start event"),
        }
    }

    #[test]
    fn test_activity_event_start_json_structure() {
        let event = ActivityEvent::Start {
            id: 123,
            kind: ActivityKind::Build,
            name: "test-package".to_string(),
            parent: Some(456),
            detail: Some("building".to_string()),
            timestamp: SystemTime::UNIX_EPOCH,
        };

        let json_str = serde_json::to_string(&event).unwrap();
        let expected = r#"{"type":"start","id":123,"kind":"build","name":"test-package","parent":456,"detail":"building","timestamp":"1970-01-01T00:00:00.000000000Z"}"#;
        assert_eq!(json_str, expected);

        let roundtrip: ActivityEvent = serde_json::from_str(&json_str).unwrap();
        match roundtrip {
            ActivityEvent::Start {
                id,
                kind,
                name,
                parent,
                detail,
                ..
            } => {
                assert_eq!(id, 123);
                assert_eq!(kind, ActivityKind::Build);
                assert_eq!(name, "test-package");
                assert_eq!(parent, Some(456));
                assert_eq!(detail, Some("building".to_string()));
            }
            _ => panic!("Expected Start event"),
        }
    }

    #[test]
    fn test_activity_event_start_optional_fields_omitted() {
        let event = ActivityEvent::Start {
            id: 123,
            kind: ActivityKind::Fetch,
            name: "test".to_string(),
            parent: None,
            detail: None,
            timestamp: SystemTime::UNIX_EPOCH,
        };

        let json_str = serde_json::to_string(&event).unwrap();
        let expected = r#"{"type":"start","id":123,"kind":"fetch","name":"test","timestamp":"1970-01-01T00:00:00.000000000Z"}"#;
        assert_eq!(json_str, expected);
    }

    #[test]
    fn test_activity_event_complete_json_structure() {
        let test_cases = [
            (ActivityOutcome::Success, r#"{"type":"complete","id":789,"outcome":"success","timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
            (ActivityOutcome::Failed, r#"{"type":"complete","id":789,"outcome":"failed","timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
            (ActivityOutcome::Cancelled, r#"{"type":"complete","id":789,"outcome":"cancelled","timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
        ];

        for (outcome, expected) in test_cases {
            let event = ActivityEvent::Complete {
                id: 789,
                outcome,
                timestamp: SystemTime::UNIX_EPOCH,
            };

            let json_str = serde_json::to_string(&event).unwrap();
            assert_eq!(json_str, expected);

            let roundtrip: ActivityEvent = serde_json::from_str(&json_str).unwrap();
            match roundtrip {
                ActivityEvent::Complete {
                    id,
                    outcome: rt_outcome,
                    ..
                } => {
                    assert_eq!(id, 789);
                    assert_eq!(rt_outcome, outcome);
                }
                _ => panic!("Expected Complete event"),
            }
        }
    }

    #[test]
    fn test_activity_event_progress_determinate_json_structure() {
        let event = ActivityEvent::Progress {
            id: 999,
            progress: ProgressState::Determinate {
                current: 50,
                total: 100,
                unit: Some(ProgressUnit::Bytes),
            },
            timestamp: SystemTime::UNIX_EPOCH,
        };

        let json_str = serde_json::to_string(&event).unwrap();
        let expected = r#"{"type":"progress","id":999,"progress":{"kind":"determinate","current":50,"total":100,"unit":"bytes"},"timestamp":"1970-01-01T00:00:00.000000000Z"}"#;
        assert_eq!(json_str, expected);

        let roundtrip: ActivityEvent = serde_json::from_str(&json_str).unwrap();
        match roundtrip {
            ActivityEvent::Progress { id, progress, .. } => {
                assert_eq!(id, 999);
                match progress {
                    ProgressState::Determinate {
                        current,
                        total,
                        unit,
                    } => {
                        assert_eq!(current, 50);
                        assert_eq!(total, 100);
                        assert_eq!(unit, Some(ProgressUnit::Bytes));
                    }
                    _ => panic!("Expected Determinate progress"),
                }
            }
            _ => panic!("Expected Progress event"),
        }
    }

    #[test]
    fn test_activity_event_progress_indeterminate_json_structure() {
        let event = ActivityEvent::Progress {
            id: 888,
            progress: ProgressState::Indeterminate {
                current: 42,
                unit: Some(ProgressUnit::Items),
            },
            timestamp: SystemTime::UNIX_EPOCH,
        };

        let json_str = serde_json::to_string(&event).unwrap();
        let expected = r#"{"type":"progress","id":888,"progress":{"kind":"indeterminate","current":42,"unit":"items"},"timestamp":"1970-01-01T00:00:00.000000000Z"}"#;
        assert_eq!(json_str, expected);

        let roundtrip: ActivityEvent = serde_json::from_str(&json_str).unwrap();
        match roundtrip {
            ActivityEvent::Progress { id, progress, .. } => {
                assert_eq!(id, 888);
                match progress {
                    ProgressState::Indeterminate { current, unit } => {
                        assert_eq!(current, 42);
                        assert_eq!(unit, Some(ProgressUnit::Items));
                    }
                    _ => panic!("Expected Indeterminate progress"),
                }
            }
            _ => panic!("Expected Progress event"),
        }
    }

    #[test]
    fn test_activity_event_phase_json_structure() {
        let event = ActivityEvent::Phase {
            id: 111,
            phase: "configure".to_string(),
            timestamp: SystemTime::UNIX_EPOCH,
        };

        let json_str = serde_json::to_string(&event).unwrap();
        let expected = r#"{"type":"phase","id":111,"phase":"configure","timestamp":"1970-01-01T00:00:00.000000000Z"}"#;
        assert_eq!(json_str, expected);

        let roundtrip: ActivityEvent = serde_json::from_str(&json_str).unwrap();
        match roundtrip {
            ActivityEvent::Phase { id, phase, .. } => {
                assert_eq!(id, 111);
                assert_eq!(phase, "configure");
            }
            _ => panic!("Expected Phase event"),
        }
    }

    #[test]
    fn test_activity_event_log_json_structure() {
        let event = ActivityEvent::Log {
            id: 222,
            line: "Building target...".to_string(),
            is_error: false,
            timestamp: SystemTime::UNIX_EPOCH,
        };

        let json_str = serde_json::to_string(&event).unwrap();
        let expected = r#"{"type":"log","id":222,"line":"Building target...","is_error":false,"timestamp":"1970-01-01T00:00:00.000000000Z"}"#;
        assert_eq!(json_str, expected);

        let error_event = ActivityEvent::Log {
            id: 333,
            line: "Error: compilation failed".to_string(),
            is_error: true,
            timestamp: SystemTime::UNIX_EPOCH,
        };

        let error_json_str = serde_json::to_string(&error_event).unwrap();
        let expected_error = r#"{"type":"log","id":333,"line":"Error: compilation failed","is_error":true,"timestamp":"1970-01-01T00:00:00.000000000Z"}"#;
        assert_eq!(error_json_str, expected_error);
    }

    #[test]
    fn test_activity_event_log_is_error_defaults_to_false() {
        let json_str = r#"{"type":"log","id":444,"line":"some log","timestamp":"1970-01-01T00:00:00.000000000Z"}"#;

        let event: ActivityEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ActivityEvent::Log { is_error, .. } => {
                assert_eq!(is_error, false);
            }
            _ => panic!("Expected Log event"),
        }
    }

    #[test]
    fn test_activity_event_message_json_structure() {
        let test_cases = [
            (LogLevel::Error, r#"{"type":"message","level":"error","text":"Test message","timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
            (LogLevel::Warn, r#"{"type":"message","level":"warn","text":"Test message","timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
            (LogLevel::Info, r#"{"type":"message","level":"info","text":"Test message","timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
            (LogLevel::Debug, r#"{"type":"message","level":"debug","text":"Test message","timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
            (LogLevel::Trace, r#"{"type":"message","level":"trace","text":"Test message","timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
        ];

        for (level, expected) in test_cases {
            let event = ActivityEvent::Message {
                level,
                text: "Test message".to_string(),
                timestamp: SystemTime::UNIX_EPOCH,
            };

            let json_str = serde_json::to_string(&event).unwrap();
            assert_eq!(json_str, expected);

            let roundtrip: ActivityEvent = serde_json::from_str(&json_str).unwrap();
            match roundtrip {
                ActivityEvent::Message {
                    level: rt_level,
                    text,
                    ..
                } => {
                    assert_eq!(rt_level, level);
                    assert_eq!(text, "Test message");
                }
                _ => panic!("Expected Message event"),
            }
        }
    }

    #[test]
    fn test_all_activity_kinds_serialize() {
        let test_cases = [
            (ActivityKind::Build, r#"{"type":"start","id":1,"kind":"build","name":"test","timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
            (ActivityKind::Fetch, r#"{"type":"start","id":1,"kind":"fetch","name":"test","timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
            (ActivityKind::Evaluate, r#"{"type":"start","id":1,"kind":"evaluate","name":"test","timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
            (ActivityKind::Task, r#"{"type":"start","id":1,"kind":"task","name":"test","timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
            (ActivityKind::Command, r#"{"type":"start","id":1,"kind":"command","name":"test","timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
            (ActivityKind::Operation, r#"{"type":"start","id":1,"kind":"operation","name":"test","timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
        ];

        for (kind, expected) in test_cases {
            let event = ActivityEvent::Start {
                id: 1,
                kind: kind.clone(),
                name: "test".to_string(),
                parent: None,
                detail: None,
                timestamp: SystemTime::UNIX_EPOCH,
            };

            let json_str = serde_json::to_string(&event).unwrap();
            assert_eq!(json_str, expected);

            let roundtrip: ActivityEvent = serde_json::from_str(&json_str).unwrap();
            match roundtrip {
                ActivityEvent::Start {
                    kind: rt_kind, ..
                } => {
                    assert_eq!(rt_kind, kind);
                }
                _ => panic!("Expected Start event"),
            }
        }
    }

    #[test]
    fn test_progress_unit_serialization() {
        let test_cases = [
            (ProgressUnit::Bytes, r#"{"type":"progress","id":1,"progress":{"kind":"indeterminate","current":10,"unit":"bytes"},"timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
            (ProgressUnit::Files, r#"{"type":"progress","id":1,"progress":{"kind":"indeterminate","current":10,"unit":"files"},"timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
            (ProgressUnit::Items, r#"{"type":"progress","id":1,"progress":{"kind":"indeterminate","current":10,"unit":"items"},"timestamp":"1970-01-01T00:00:00.000000000Z"}"#),
        ];

        for (unit, expected) in test_cases {
            let event = ActivityEvent::Progress {
                id: 1,
                progress: ProgressState::Indeterminate {
                    current: 10,
                    unit: Some(unit),
                },
                timestamp: SystemTime::UNIX_EPOCH,
            };

            let json_str = serde_json::to_string(&event).unwrap();
            assert_eq!(json_str, expected);

            let roundtrip: ActivityEvent = serde_json::from_str(&json_str).unwrap();
            match roundtrip {
                ActivityEvent::Progress { progress, .. } => match progress {
                    ProgressState::Indeterminate { unit: rt_unit, .. } => {
                        assert_eq!(rt_unit, Some(unit));
                    }
                    _ => panic!("Expected Indeterminate progress"),
                },
                _ => panic!("Expected Progress event"),
            }
        }
    }

    #[tokio::test]
    async fn test_deref_to_span() {
        use tracing::Instrument;

        let (mut rx, _guard) = setup_test();

        async fn example_async_work() -> u32 {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            42
        }

        let activity = Activity::build("async-task");
        let parent_id = activity.id();

        // Use .span() to get a span for .instrument()
        let result = example_async_work().instrument(activity.span()).await;
        assert_eq!(result, 42);

        // Verify the activity was created
        let start = rx.recv().await.unwrap();
        match start {
            ActivityEvent::Start { id, name, .. } => {
                assert_eq!(id, parent_id);
                assert_eq!(name, "async-task");
            }
            _ => panic!("Expected Start event"),
        }
    }

    #[tokio::test]
    async fn test_in_scope_via_deref() {
        let (mut rx, _guard) = setup_test();

        let activity = Activity::build("scoped-task");
        let parent_id = activity.id();

        // Deref allows us to use span's in_scope() directly
        let result = activity.in_scope(|| {
            // Create nested activity within scope
            let _nested = Activity::fetch("scoped-fetch");
            42
        });
        assert_eq!(result, 42);

        // Verify parent start
        let start = rx.recv().await.unwrap();
        match start {
            ActivityEvent::Start { id, name, .. } => {
                assert_eq!(id, parent_id);
                assert_eq!(name, "scoped-task");
            }
            _ => panic!("Expected Start event"),
        }

        // Verify nested activity has parent set
        let nested_start = rx.recv().await.unwrap();
        match nested_start {
            ActivityEvent::Start { parent, name, .. } => {
                assert_eq!(parent, Some(parent_id));
                assert_eq!(name, "scoped-fetch");
            }
            _ => panic!("Expected Start event"),
        }
    }

    #[tokio::test]
    async fn test_enter_via_deref() {
        let (mut rx, _guard) = setup_test();

        let activity = Activity::build("entered-task");
        let parent_id = activity.id();

        // Deref allows us to use span's enter() directly
        {
            let _guard = activity.enter();
            // Create nested activity while entered
            let _nested = Activity::fetch("nested-in-guard");
        }

        // Verify parent start
        let start = rx.recv().await.unwrap();
        match start {
            ActivityEvent::Start { id, .. } => {
                assert_eq!(id, parent_id);
            }
            _ => panic!("Expected Start event"),
        }

        // Verify nested activity has parent set
        let nested_start = rx.recv().await.unwrap();
        match nested_start {
            ActivityEvent::Start { parent, .. } => {
                assert_eq!(parent, Some(parent_id));
            }
            _ => panic!("Expected Start event"),
        }
    }
}
