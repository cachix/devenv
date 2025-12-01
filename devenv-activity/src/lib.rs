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
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;
use tokio::sync::mpsc;
use tracing::{Level, Span, span};

// Global sender for activity events (installed by ActivityHandle::install())
static ACTIVITY_SENDER: OnceLock<mpsc::UnboundedSender<ActivityEvent>> = OnceLock::new();

/// Send an activity event to the registered channel (if any)
fn send_activity_event(event: ActivityEvent) {
    if let Some(tx) = ACTIVITY_SENDER.get() {
        let _ = tx.send(event);
    }
}

// Re-export for convenience
pub use tracing_subscriber::Registry;

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

// Thread-local stack for tracking current Activity IDs (for parent detection)
thread_local! {
    static ACTIVITY_STACK: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// All activity events
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
        timestamp: Timestamp,
    },

    /// Activity completed
    Complete {
        id: u64,
        outcome: ActivityOutcome,
        timestamp: Timestamp,
    },

    /// Progress update
    Progress {
        id: u64,
        progress: ProgressState,
        timestamp: Timestamp,
    },

    /// Phase/step change
    Phase {
        id: u64,
        phase: String,
        timestamp: Timestamp,
    },

    /// Log line from activity
    Log {
        id: u64,
        line: String,
        #[serde(default)]
        is_error: bool,
        timestamp: Timestamp,
    },

    /// Message not tied to an activity
    Message {
        level: LogLevel,
        text: String,
        timestamp: Timestamp,
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
    outcome: std::sync::Mutex<ActivityOutcome>,
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
    ///
    /// Uses the thread-local activity stack to determine the parent,
    /// just like regular activities. The external ID allows correlating
    /// with Nix's activity lifecycle events.
    pub fn start_with_id(
        id: u64,
        kind: ActivityKind,
        name: String,
        detail: Option<String>,
    ) -> Self {
        let parent = get_current_activity_id();
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
        let span = span!(Level::TRACE, "activity", activity_id = id);

        // Emit start event to tracing
        let kind_str = kind.to_string();
        tracing::trace!(
            target: "devenv::activity",
            activity_id = id,
            event_type = "start",
            kind = %kind_str,
            name = %name,
            parent = ?parent,
            detail = ?detail,
        );

        // Send to channel if installed
        send_activity_event(ActivityEvent::Start {
            id,
            kind: kind.clone(),
            name: name.clone(),
            parent,
            detail: detail.clone(),
            timestamp: Timestamp::now(),
        });

        // Push to activity stack for parent tracking
        ACTIVITY_STACK.with(|stack| stack.borrow_mut().push(id));

        Self {
            span,
            id,
            outcome: std::sync::Mutex::new(ActivityOutcome::Success),
        }
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

        send_activity_event(ActivityEvent::Progress {
            id: self.id,
            progress: ProgressState::Determinate {
                current,
                total,
                unit: None,
            },
            timestamp: Timestamp::now(),
        });
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

        send_activity_event(ActivityEvent::Progress {
            id: self.id,
            progress: ProgressState::Determinate {
                current,
                total,
                unit: Some(ProgressUnit::Bytes),
            },
            timestamp: Timestamp::now(),
        });
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

        send_activity_event(ActivityEvent::Progress {
            id: self.id,
            progress: ProgressState::Indeterminate {
                current,
                unit: None,
            },
            timestamp: Timestamp::now(),
        });
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

        send_activity_event(ActivityEvent::Phase {
            id: self.id,
            phase: phase_str,
            timestamp: Timestamp::now(),
        });
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

        send_activity_event(ActivityEvent::Log {
            id: self.id,
            line: line_str,
            is_error: false,
            timestamp: Timestamp::now(),
        });
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

        send_activity_event(ActivityEvent::Log {
            id: self.id,
            line: line_str,
            is_error: true,
            timestamp: Timestamp::now(),
        });
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
            outcome: std::sync::Mutex::new(outcome),
        }
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        // Emit complete event to tracing
        let outcome = self
            .outcome
            .lock()
            .map(|o| *o)
            .unwrap_or(ActivityOutcome::Success);
        let outcome_str = match outcome {
            ActivityOutcome::Success => "success",
            ActivityOutcome::Failed => "failed",
            ActivityOutcome::Cancelled => "cancelled",
        };
        tracing::trace!(
            target: "devenv::activity",
            activity_id = self.id,
            event_type = "complete",
            outcome = %outcome_str,
        );

        // Send to channel if installed
        send_activity_event(ActivityEvent::Complete {
            id: self.id,
            outcome,
            timestamp: Timestamp::now(),
        });

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

    send_activity_event(ActivityEvent::Message {
        level,
        text: text_str,
        timestamp: Timestamp::now(),
    });
}
// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Handle for registering the activity event channel
pub struct ActivityHandle {
    tx: mpsc::UnboundedSender<ActivityEvent>,
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
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
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
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
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
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
        };

        let json_str = serde_json::to_string(&event).unwrap();
        let expected = r#"{"type":"start","id":123,"kind":"fetch","name":"test","timestamp":"1970-01-01T00:00:00.000000000Z"}"#;
        assert_eq!(json_str, expected);
    }

    #[test]
    fn test_activity_event_complete_json_structure() {
        let test_cases = [
            (
                ActivityOutcome::Success,
                r#"{"type":"complete","id":789,"outcome":"success","timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
            (
                ActivityOutcome::Failed,
                r#"{"type":"complete","id":789,"outcome":"failed","timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
            (
                ActivityOutcome::Cancelled,
                r#"{"type":"complete","id":789,"outcome":"cancelled","timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
        ];

        for (outcome, expected) in test_cases {
            let event = ActivityEvent::Complete {
                id: 789,
                outcome,
                timestamp: Timestamp(SystemTime::UNIX_EPOCH),
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
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
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
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
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
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
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
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
        };

        let json_str = serde_json::to_string(&event).unwrap();
        let expected = r#"{"type":"log","id":222,"line":"Building target...","is_error":false,"timestamp":"1970-01-01T00:00:00.000000000Z"}"#;
        assert_eq!(json_str, expected);

        let error_event = ActivityEvent::Log {
            id: 333,
            line: "Error: compilation failed".to_string(),
            is_error: true,
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
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
            (
                LogLevel::Error,
                r#"{"type":"message","level":"error","text":"Test message","timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
            (
                LogLevel::Warn,
                r#"{"type":"message","level":"warn","text":"Test message","timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
            (
                LogLevel::Info,
                r#"{"type":"message","level":"info","text":"Test message","timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
            (
                LogLevel::Debug,
                r#"{"type":"message","level":"debug","text":"Test message","timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
            (
                LogLevel::Trace,
                r#"{"type":"message","level":"trace","text":"Test message","timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
        ];

        for (level, expected) in test_cases {
            let event = ActivityEvent::Message {
                level,
                text: "Test message".to_string(),
                timestamp: Timestamp(SystemTime::UNIX_EPOCH),
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
            (
                ActivityKind::Build,
                r#"{"type":"start","id":1,"kind":"build","name":"test","timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
            (
                ActivityKind::Fetch,
                r#"{"type":"start","id":1,"kind":"fetch","name":"test","timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
            (
                ActivityKind::Evaluate,
                r#"{"type":"start","id":1,"kind":"evaluate","name":"test","timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
            (
                ActivityKind::Task,
                r#"{"type":"start","id":1,"kind":"task","name":"test","timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
            (
                ActivityKind::Command,
                r#"{"type":"start","id":1,"kind":"command","name":"test","timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
            (
                ActivityKind::Operation,
                r#"{"type":"start","id":1,"kind":"operation","name":"test","timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
        ];

        for (kind, expected) in test_cases {
            let event = ActivityEvent::Start {
                id: 1,
                kind: kind.clone(),
                name: "test".to_string(),
                parent: None,
                detail: None,
                timestamp: Timestamp(SystemTime::UNIX_EPOCH),
            };

            let json_str = serde_json::to_string(&event).unwrap();
            assert_eq!(json_str, expected);

            let roundtrip: ActivityEvent = serde_json::from_str(&json_str).unwrap();
            match roundtrip {
                ActivityEvent::Start { kind: rt_kind, .. } => {
                    assert_eq!(rt_kind, kind);
                }
                _ => panic!("Expected Start event"),
            }
        }
    }

    #[test]
    fn test_progress_unit_serialization() {
        let test_cases = [
            (
                ProgressUnit::Bytes,
                r#"{"type":"progress","id":1,"progress":{"kind":"indeterminate","current":10,"unit":"bytes"},"timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
            (
                ProgressUnit::Files,
                r#"{"type":"progress","id":1,"progress":{"kind":"indeterminate","current":10,"unit":"files"},"timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
            (
                ProgressUnit::Items,
                r#"{"type":"progress","id":1,"progress":{"kind":"indeterminate","current":10,"unit":"items"},"timestamp":"1970-01-01T00:00:00.000000000Z"}"#,
            ),
        ];

        for (unit, expected) in test_cases {
            let event = ActivityEvent::Progress {
                id: 1,
                progress: ProgressState::Indeterminate {
                    current: 10,
                    unit: Some(unit),
                },
                timestamp: Timestamp(SystemTime::UNIX_EPOCH),
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
}
