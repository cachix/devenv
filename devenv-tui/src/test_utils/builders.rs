//! Builder patterns for creating test data

use crate::{
    Activity, ActivityVariant, BuildActivity, DownloadActivity, QueryActivity, TaskActivity,
    TaskDisplayStatus,
};
use devenv_activity::{ActivityKind, ActivityOutcome, ProgressState, ProgressUnit};
use std::time::Instant;

/// Builder for creating Activity instances in tests
pub struct ActivityBuilder {
    id: u64,
    name: String,
    short_name: String,
    parent: Option<u64>,
    children: Vec<u64>,
    start_time: Instant,
    completed: Option<(ActivityOutcome, Instant)>,
    detail: Option<String>,
    kind: ActivityKind,
    variant: ActivityVariant,
    progress: Option<ProgressState>,
}

impl Default for ActivityBuilder {
    fn default() -> Self {
        Self {
            id: 1,
            name: "test-activity".to_string(),
            short_name: "test-activity".to_string(),
            parent: None,
            children: Vec::new(),
            start_time: Instant::now(),
            completed: None,
            detail: None,
            kind: ActivityKind::Build,
            variant: ActivityVariant::Build(BuildActivity { phase: None }),
            progress: None,
        }
    }
}

impl ActivityBuilder {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn short_name(mut self, short_name: impl Into<String>) -> Self {
        self.short_name = short_name.into();
        self
    }

    pub fn parent(mut self, parent: u64) -> Self {
        self.parent = Some(parent);
        self
    }

    pub fn completed(mut self, outcome: ActivityOutcome) -> Self {
        self.completed = Some((outcome, Instant::now()));
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn variant(mut self, variant: ActivityVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn progress(mut self, current: u64, total: u64) -> Self {
        self.progress = Some(ProgressState::Determinate {
            current,
            total,
            unit: Some(ProgressUnit::Bytes),
        });
        self
    }

    pub fn build(self) -> Activity {
        Activity {
            id: self.id,
            name: self.name,
            short_name: self.short_name,
            parent: self.parent,
            children: self.children,
            start_time: self.start_time,
            completed: self.completed,
            detail: self.detail,
            kind: self.kind,
            variant: self.variant,
            progress: self.progress,
        }
    }

    // Convenience methods for specific activity types

    pub fn build_activity(mut self) -> Self {
        self.kind = ActivityKind::Build;
        self.variant = ActivityVariant::Build(BuildActivity { phase: None });
        self
    }

    pub fn build_activity_with_phase(mut self, phase: impl Into<String>) -> Self {
        self.kind = ActivityKind::Build;
        self.variant = ActivityVariant::Build(BuildActivity {
            phase: Some(phase.into()),
        });
        self
    }

    pub fn download_activity(mut self) -> Self {
        self.kind = ActivityKind::Fetch;
        self.variant = ActivityVariant::Download(DownloadActivity { substituter: None });
        self
    }

    pub fn download_activity_with_substituter(mut self, substituter: impl Into<String>) -> Self {
        self.kind = ActivityKind::Fetch;
        self.variant = ActivityVariant::Download(DownloadActivity {
            substituter: Some(substituter.into()),
        });
        self
    }

    pub fn query_activity_with_substituter(mut self, substituter: impl Into<String>) -> Self {
        self.kind = ActivityKind::Operation;
        self.variant = ActivityVariant::Query(QueryActivity {
            substituter: Some(substituter.into()),
        });
        self
    }

    pub fn task_activity(mut self, status: TaskDisplayStatus) -> Self {
        self.kind = ActivityKind::Task;
        self.variant = ActivityVariant::Task(TaskActivity { status });
        self
    }

    pub fn evaluating_activity(mut self) -> Self {
        self.kind = ActivityKind::Evaluate;
        self.variant = ActivityVariant::Evaluating;
        self
    }

    pub fn fetch_tree_activity(mut self) -> Self {
        self.kind = ActivityKind::Fetch;
        self.variant = ActivityVariant::FetchTree;
        self
    }
}
