//! Test helpers for constructing `ActivityEvent`s with sensible defaults.
//!
//! Available when the `test-helpers` feature is enabled (and always in
//! this crate's own tests via `cfg(test)`). Consumers add:
//!
//! ```toml
//! [dev-dependencies]
//! devenv-activity = { workspace = true, features = ["test-helpers"] }
//! ```
//!
//! Each event kind has a brief helper for the common case and a `_with`
//! variant that exposes parent/url/detail/level for tests that exercise
//! hierarchy or non-default configuration. String arguments accept both
//! `&str` and `String` via `impl Into<String>`.

use crate::events::{
    ActivityEvent, ActivityLevel, ActivityOutcome, Build, Command, EvalOp, Evaluate, Fetch,
    FetchKind, Message, Operation, Process, ProcessStatus, Task, TaskInfo,
};
use crate::timestamp::Timestamp;

// --- Task ---

pub fn task_hierarchy(tasks: Vec<TaskInfo>, edges: Vec<(u64, u64)>) -> ActivityEvent {
    ActivityEvent::Task(Task::Hierarchy {
        tasks,
        edges,
        timestamp: Timestamp::now(),
    })
}

pub fn task_hierarchy_single(
    id: u64,
    name: impl Into<String>,
    parent: Option<u64>,
    show_output: bool,
    is_process: bool,
) -> ActivityEvent {
    let edges = parent.map(|p| vec![(p, id)]).unwrap_or_default();
    task_hierarchy(
        vec![TaskInfo {
            id,
            name: name.into(),
            show_output,
            is_process,
        }],
        edges,
    )
}

pub fn task_start(id: u64) -> ActivityEvent {
    ActivityEvent::Task(Task::Start {
        id,
        timestamp: Timestamp::now(),
    })
}

pub fn task_log(id: u64, line: impl Into<String>, is_error: bool) -> ActivityEvent {
    ActivityEvent::Task(Task::Log {
        id,
        line: line.into(),
        is_error,
        timestamp: Timestamp::now(),
    })
}

pub fn task_complete(id: u64, outcome: ActivityOutcome) -> ActivityEvent {
    ActivityEvent::Task(Task::Complete {
        id,
        outcome,
        timestamp: Timestamp::now(),
    })
}

pub fn task_progress(id: u64, done: u64, expected: u64) -> ActivityEvent {
    ActivityEvent::Task(Task::Progress {
        id,
        done,
        expected,
        timestamp: Timestamp::now(),
    })
}

// --- Build ---

pub fn build_start(id: u64, name: impl Into<String>) -> ActivityEvent {
    build_start_with(id, name, None)
}

pub fn build_start_with(id: u64, name: impl Into<String>, parent: Option<u64>) -> ActivityEvent {
    ActivityEvent::Build(Build::Start {
        id,
        name: name.into(),
        parent,
        derivation_path: None,
        timestamp: Timestamp::now(),
    })
}

pub fn build_phase(id: u64, phase: impl Into<String>) -> ActivityEvent {
    ActivityEvent::Build(Build::Phase {
        id,
        phase: phase.into(),
        timestamp: Timestamp::now(),
    })
}

pub fn build_log(id: u64, line: impl Into<String>, is_error: bool) -> ActivityEvent {
    ActivityEvent::Build(Build::Log {
        id,
        line: line.into(),
        is_error,
        timestamp: Timestamp::now(),
    })
}

pub fn build_complete(id: u64, outcome: ActivityOutcome) -> ActivityEvent {
    ActivityEvent::Build(Build::Complete {
        id,
        outcome,
        timestamp: Timestamp::now(),
    })
}

// --- Command ---

pub fn command_start(id: u64, name: impl Into<String>) -> ActivityEvent {
    ActivityEvent::Command(Command::Start {
        id,
        name: name.into(),
        parent: None,
        command: None,
        timestamp: Timestamp::now(),
    })
}

pub fn command_log(id: u64, line: impl Into<String>, is_error: bool) -> ActivityEvent {
    ActivityEvent::Command(Command::Log {
        id,
        line: line.into(),
        is_error,
        timestamp: Timestamp::now(),
    })
}

pub fn command_complete(id: u64, outcome: ActivityOutcome) -> ActivityEvent {
    ActivityEvent::Command(Command::Complete {
        id,
        outcome,
        timestamp: Timestamp::now(),
    })
}

// --- Process ---

pub fn process_start(id: u64, name: impl Into<String>) -> ActivityEvent {
    process_start_with(id, name, None, ActivityLevel::Info)
}

pub fn process_start_with(
    id: u64,
    name: impl Into<String>,
    parent: Option<u64>,
    level: ActivityLevel,
) -> ActivityEvent {
    ActivityEvent::Process(Process::Start {
        id,
        name: name.into(),
        parent,
        command: None,
        ports: vec![],
        ready_probe: None,
        level,
        timestamp: Timestamp::now(),
    })
}

pub fn process_log(id: u64, line: impl Into<String>, is_error: bool) -> ActivityEvent {
    ActivityEvent::Process(Process::Log {
        id,
        line: line.into(),
        is_error,
        timestamp: Timestamp::now(),
    })
}

pub fn process_status(id: u64, status: ProcessStatus) -> ActivityEvent {
    ActivityEvent::Process(Process::Status {
        id,
        status,
        timestamp: Timestamp::now(),
    })
}

pub fn process_complete(id: u64, outcome: ActivityOutcome) -> ActivityEvent {
    ActivityEvent::Process(Process::Complete {
        id,
        outcome,
        timestamp: Timestamp::now(),
    })
}

// --- Operation ---

pub fn operation_start(id: u64, name: impl Into<String>) -> ActivityEvent {
    operation_start_with(id, name, None, None, ActivityLevel::Info)
}

pub fn operation_start_with(
    id: u64,
    name: impl Into<String>,
    parent: Option<u64>,
    detail: Option<&str>,
    level: ActivityLevel,
) -> ActivityEvent {
    ActivityEvent::Operation(Operation::Start {
        id,
        name: name.into(),
        parent,
        detail: detail.map(str::to_string),
        level,
        timestamp: Timestamp::now(),
    })
}

pub fn operation_log(id: u64, line: impl Into<String>, is_error: bool) -> ActivityEvent {
    ActivityEvent::Operation(Operation::Log {
        id,
        line: line.into(),
        is_error,
        timestamp: Timestamp::now(),
    })
}

pub fn operation_progress(id: u64, done: u64, expected: u64) -> ActivityEvent {
    operation_progress_with(id, done, expected, None)
}

pub fn operation_progress_with(
    id: u64,
    done: u64,
    expected: u64,
    detail: Option<&str>,
) -> ActivityEvent {
    ActivityEvent::Operation(Operation::Progress {
        id,
        done,
        expected,
        detail: detail.map(str::to_string),
        timestamp: Timestamp::now(),
    })
}

pub fn operation_complete(id: u64, outcome: ActivityOutcome) -> ActivityEvent {
    ActivityEvent::Operation(Operation::Complete {
        id,
        outcome,
        timestamp: Timestamp::now(),
    })
}

// --- Fetch ---

pub fn fetch_start(id: u64, kind: FetchKind, name: impl Into<String>) -> ActivityEvent {
    fetch_start_with(id, kind, name, None, None)
}

pub fn fetch_start_with(
    id: u64,
    kind: FetchKind,
    name: impl Into<String>,
    parent: Option<u64>,
    url: Option<&str>,
) -> ActivityEvent {
    ActivityEvent::Fetch(Fetch::Start {
        id,
        kind,
        name: name.into(),
        parent,
        url: url.map(str::to_string),
        timestamp: Timestamp::now(),
    })
}

pub fn fetch_progress(id: u64, current: u64, total: Option<u64>) -> ActivityEvent {
    ActivityEvent::Fetch(Fetch::Progress {
        id,
        current,
        total,
        timestamp: Timestamp::now(),
    })
}

pub fn fetch_complete(id: u64, outcome: ActivityOutcome) -> ActivityEvent {
    ActivityEvent::Fetch(Fetch::Complete {
        id,
        outcome,
        timestamp: Timestamp::now(),
    })
}

// --- Evaluate ---

pub fn evaluate_start(id: u64, name: impl Into<String>, level: ActivityLevel) -> ActivityEvent {
    evaluate_start_with(id, name, level, None)
}

pub fn evaluate_start_with(
    id: u64,
    name: impl Into<String>,
    level: ActivityLevel,
    parent: Option<u64>,
) -> ActivityEvent {
    ActivityEvent::Evaluate(Evaluate::Start {
        id,
        name: name.into(),
        level,
        parent,
        timestamp: Timestamp::now(),
    })
}

pub fn evaluate_log(id: u64, line: impl Into<String>) -> ActivityEvent {
    ActivityEvent::Evaluate(Evaluate::Log {
        id,
        line: line.into(),
        timestamp: Timestamp::now(),
    })
}

pub fn evaluate_op(id: u64, op: EvalOp) -> ActivityEvent {
    ActivityEvent::Evaluate(Evaluate::Op {
        id,
        op,
        timestamp: Timestamp::now(),
    })
}

pub fn evaluate_complete(id: u64, outcome: ActivityOutcome) -> ActivityEvent {
    ActivityEvent::Evaluate(Evaluate::Complete {
        id,
        outcome,
        timestamp: Timestamp::now(),
    })
}

// --- Message ---

pub fn message(level: ActivityLevel, text: impl Into<String>) -> ActivityEvent {
    message_with(0, level, text, None)
}

pub fn message_with(
    id: u64,
    level: ActivityLevel,
    text: impl Into<String>,
    parent: Option<u64>,
) -> ActivityEvent {
    message_with_details(id, level, text, None, parent)
}

pub fn message_with_details(
    id: u64,
    level: ActivityLevel,
    text: impl Into<String>,
    details: Option<&str>,
    parent: Option<u64>,
) -> ActivityEvent {
    ActivityEvent::Message(Message {
        id,
        level,
        text: text.into(),
        details: details.map(str::to_string),
        parent,
        timestamp: Timestamp::now(),
    })
}
