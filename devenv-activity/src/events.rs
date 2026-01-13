//! Activity event types for the devenv activity tracking system.

use serde::{Deserialize, Serialize};
use valuable::Valuable;

use crate::Timestamp;

/// All activity events - activity-first design
#[derive(Debug, Clone, Serialize, Deserialize, Valuable)]
#[serde(tag = "activity_kind", rename_all = "lowercase")]
pub enum ActivityEvent {
    Build(Build),
    Fetch(Fetch),
    Evaluate(Evaluate),
    Task(Task),
    Command(Command),
    Operation(Operation),
    Message(Message),
    /// Aggregate expected counts announcement from Nix
    SetExpected(SetExpected),
    Shell(Shell),
}

/// Expected count announcement for aggregate activity tracking.
/// Nix emits these events to announce how many items/bytes are expected
/// before individual activities start (e.g., "expect 10 downloads").
#[derive(Debug, Clone, Serialize, Deserialize, Valuable)]
pub struct SetExpected {
    /// The category of activity this expectation applies to
    pub category: ExpectedCategory,
    /// The expected count (items for builds, bytes for downloads)
    pub expected: u64,
    pub timestamp: Timestamp,
}

/// Categories for expected count tracking
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Valuable)]
#[serde(rename_all = "lowercase")]
pub enum ExpectedCategory {
    /// Build activities (derivations to build)
    Build,
    /// Download activities (store paths to download, bytes to transfer)
    Download,
}

/// Build activity events - has Phase, Progress, Log
#[derive(Debug, Clone, Serialize, Deserialize, Valuable)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Build {
    /// Build is queued, waiting for a build slot
    Queued {
        #[serde(alias = "activity_id")]
        id: u64,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        derivation_path: Option<String>,
        timestamp: Timestamp,
    },
    /// Build has started running
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
#[derive(Debug, Clone, Serialize, Deserialize, Valuable)]
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Valuable)]
#[serde(rename_all = "lowercase")]
pub enum FetchKind {
    /// Downloading store paths from substituter
    Download,
    /// Querying path info from cache
    Query,
    /// Fetching git trees/flake inputs
    Tree,
    /// Copying local sources to the store (e.g., flake inputs)
    Copy,
}

/// A filesystem or environment operation observed during Nix evaluation.
///
/// These operations are logged during evaluation and can be used for
/// cache invalidation and dependency tracking.
///
/// Note: Duplicated in `devenv_core::eval_op::EvalOp`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Valuable)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvalOp {
    /// Copied a file to the Nix store.
    CopiedSource {
        source: std::path::PathBuf,
        target: std::path::PathBuf,
    },
    /// Evaluated a Nix file.
    EvaluatedFile { source: std::path::PathBuf },
    /// Read a file's contents with `builtins.readFile`.
    ReadFile { source: std::path::PathBuf },
    /// List a directory's contents with `builtins.readDir`.
    ReadDir { source: std::path::PathBuf },
    /// Read an environment variable with `builtins.getEnv`.
    GetEnv { name: String },
    /// Check that a file exists with `builtins.pathExists`.
    PathExists { source: std::path::PathBuf },
    /// Used a tracked devenv string path.
    TrackedPath { source: std::path::PathBuf },
}

/// Evaluate activity events.
#[derive(Debug, Clone, Serialize, Deserialize, Valuable)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Evaluate {
    Start {
        #[serde(alias = "activity_id")]
        id: u64,
        name: String,
        level: ActivityLevel,
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
    /// Plain text log line from evaluation.
    Log {
        #[serde(alias = "activity_id")]
        id: u64,
        line: String,
        timestamp: Timestamp,
    },
    /// Structured evaluation operation (parsed from log).
    Op {
        #[serde(alias = "activity_id")]
        id: u64,
        op: EvalOp,
        timestamp: Timestamp,
    },
}

/// Information about a task in the hierarchy
#[derive(Debug, Clone, Serialize, Deserialize, Valuable)]
pub struct TaskInfo {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub show_output: bool,
    #[serde(default)]
    pub is_process: bool,
}

/// Task activity events - has Progress, Log
#[derive(Debug, Clone, Serialize, Deserialize, Valuable)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Task {
    /// Emit task hierarchy once upfront before execution
    Hierarchy {
        /// All tasks with their metadata
        tasks: Vec<TaskInfo>,
        /// Edges representing parent-child relationships: (parent_id, child_id)
        /// A task appears under its dependents (i.e., tasks that depend on it)
        edges: Vec<(u64, u64)>,
        timestamp: Timestamp,
    },
    /// Task execution has started
    Start {
        #[serde(alias = "activity_id")]
        id: u64,
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
#[derive(Debug, Clone, Serialize, Deserialize, Valuable)]
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

/// Operation activity events - generic devenv operations with log support
#[derive(Debug, Clone, Serialize, Deserialize, Valuable)]
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
        #[serde(default)]
        level: ActivityLevel,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
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

/// Message - standalone (not an activity)
#[derive(Debug, Clone, Serialize, Deserialize, Valuable)]
pub struct Message {
    pub id: u64,
    pub level: ActivityLevel,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    pub timestamp: Timestamp,
}

/// Shell activity events - interactive shell with hot-reload capability
#[derive(Debug, Clone, Serialize, Deserialize, Valuable)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Shell {
    /// Shell session started
    Start {
        #[serde(alias = "activity_id")]
        id: u64,
        /// Shell command being run (None for interactive)
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        /// Files being watched for changes
        watch_files: Vec<String>,
        timestamp: Timestamp,
    },
    /// Shell session ended
    Complete {
        #[serde(alias = "activity_id")]
        id: u64,
        outcome: ActivityOutcome,
        timestamp: Timestamp,
    },
    /// Output from the shell (PTY output)
    Output {
        #[serde(alias = "activity_id")]
        id: u64,
        /// Raw bytes from PTY (may contain ANSI escape codes)
        data: Vec<u8>,
        timestamp: Timestamp,
    },
    /// Shell is reloading due to file changes
    Reloading {
        #[serde(alias = "activity_id")]
        id: u64,
        /// Files that changed and triggered the reload
        changed_files: Vec<String>,
        timestamp: Timestamp,
    },
    /// Shell reload completed successfully
    Reloaded {
        #[serde(alias = "activity_id")]
        id: u64,
        /// Files that were changed
        changed_files: Vec<String>,
        timestamp: Timestamp,
    },
    /// Shell reload failed
    ReloadFailed {
        #[serde(alias = "activity_id")]
        id: u64,
        /// Files that triggered the reload
        changed_files: Vec<String>,
        /// Error message
        error: String,
        timestamp: Timestamp,
    },
}

/// Outcome of an activity
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, Valuable)]
#[serde(rename_all = "lowercase")]
pub enum ActivityOutcome {
    #[default]
    Success,
    Failed,
    Cancelled,
    /// Task output was already cached
    Cached,
    /// Task had no command to run
    Skipped,
    /// Task's dependency failed
    DependencyFailed,
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
    Valuable,
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

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

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
        let kinds = [
            FetchKind::Download,
            FetchKind::Query,
            FetchKind::Tree,
            FetchKind::Copy,
        ];
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
            id: 1,
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
