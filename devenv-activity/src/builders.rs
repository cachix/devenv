//! Builder types for creating activities.

use std::sync::atomic::{AtomicU64, Ordering};

use tracing::{Level, Span, span};

use crate::Timestamp;
use crate::activity::{Activity, ActivityType};
use crate::events::{
    ActivityEvent, ActivityLevel, Build, Command, Evaluate, Fetch, FetchKind, Operation, Task,
};
use crate::stack::{current_activity_id, send_activity_event};

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Generate a new activity ID.
/// Uses high bit to distinguish from Nix-generated IDs.
fn next_id() -> u64 {
    ID_COUNTER.fetch_add(1, Ordering::Relaxed) | (1 << 63)
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

/// Builder for Build activities
pub struct BuildBuilder {
    name: String,
    derivation_path: Option<String>,
    id: Option<u64>,
    parent: Option<Option<u64>>,
    level: ActivityLevel,
}

impl BuildBuilder {
    pub(crate) fn new(name: impl Into<String>) -> Self {
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

        Activity::new(span, id, ActivityType::Build)
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
    pub(crate) fn new(kind: FetchKind, name: impl Into<String>) -> Self {
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

        Activity::new(span, id, ActivityType::Fetch(self.kind))
    }
}

/// Builder for Evaluate activities
pub struct EvaluateBuilder {
    id: Option<u64>,
    parent: Option<Option<u64>>,
    level: ActivityLevel,
}

impl EvaluateBuilder {
    pub(crate) fn new() -> Self {
        Self {
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
            parent,
            timestamp: Timestamp::now(),
        }));

        Activity::new(span, id, ActivityType::Evaluate)
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
    pub(crate) fn new(name: impl Into<String>) -> Self {
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

        Activity::new(span, id, ActivityType::Task)
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
    pub(crate) fn new(name: impl Into<String>) -> Self {
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

        Activity::new(span, id, ActivityType::Command)
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
    pub(crate) fn new(name: impl Into<String>) -> Self {
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

        // NOTE: Include devenv.user_message for the legacy devenv CLI
        //
        // span! requires compile-time constant levels, so we match on each variant
        let name = self.name.as_str();
        let span = match self.level {
            ActivityLevel::Error => span!(
                Level::ERROR,
                "activity",
                activity_id = id,
                devenv.user_message = name
            ),
            ActivityLevel::Warn => span!(
                Level::WARN,
                "activity",
                activity_id = id,
                devenv.user_message = name
            ),
            ActivityLevel::Info => span!(
                Level::INFO,
                "activity",
                activity_id = id,
                devenv.user_message = name
            ),
            ActivityLevel::Debug => span!(
                Level::DEBUG,
                "activity",
                activity_id = id,
                devenv.user_message = name
            ),
            ActivityLevel::Trace => span!(
                Level::TRACE,
                "activity",
                activity_id = id,
                devenv.user_message = name
            ),
        };

        send_activity_event(ActivityEvent::Operation(Operation::Start {
            id,
            name: self.name.clone(),
            parent,
            detail: self.detail,
            timestamp: Timestamp::now(),
        }));

        Activity::new(span, id, ActivityType::Operation)
    }
}
