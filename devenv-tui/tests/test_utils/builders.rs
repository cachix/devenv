use devenv_tui::{
    Activity, ActivityVariant, BuildActivity, DownloadActivity, NixActivityState, OperationId,
    ProgressActivity, QueryActivity, TaskActivity, TaskDisplayStatus,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub struct ActivityBuilder {
    id: u64,
    operation_id: OperationId,
    name: String,
    short_name: String,
    parent_id: Option<u64>,
    state: NixActivityState,
    detail: Option<String>,
    variant: ActivityVariant,
    progress: Option<ProgressActivity>,
}

impl ActivityBuilder {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            operation_id: OperationId::from_activity_id(id),
            name: format!("Activity {}", id),
            short_name: format!("act-{}", id),
            parent_id: None,
            state: NixActivityState::Active,
            detail: None,
            variant: ActivityVariant::Unknown,
            progress: None,
        }
    }

    pub fn operation_id(mut self, operation_id: OperationId) -> Self {
        self.operation_id = operation_id;
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn short_name(mut self, short_name: impl Into<String>) -> Self {
        self.short_name = short_name.into();
        self
    }

    pub fn parent_id(mut self, parent: u64) -> Self {
        self.parent_id = Some(parent);
        self
    }

    pub fn completed(mut self, success: bool, duration_secs: u64) -> Self {
        self.state = NixActivityState::Completed {
            success,
            duration: Duration::from_secs(duration_secs),
        };
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

    pub fn progress(mut self, current: u64, total: u64, unit: impl Into<String>) -> Self {
        let percent = (current as f32 / total as f32) * 100.0;
        self.progress = Some(ProgressActivity {
            current: Some(current),
            total: Some(total),
            unit: Some(unit.into()),
            percent: Some(percent),
        });
        self
    }

    pub fn build(self) -> Activity {
        Activity {
            id: self.id,
            operation_id: self.operation_id,
            name: self.name,
            short_name: self.short_name,
            parent_id: self.parent_id,
            start_time: Instant::now(),
            state: self.state,
            detail: self.detail,
            variant: self.variant,
            progress: self.progress,
        }
    }
}

impl ActivityBuilder {
    pub fn build_activity(self) -> Self {
        self.variant(ActivityVariant::Build(BuildActivity {
            phase: None,
            log_stdout_lines: Vec::new(),
            log_stderr_lines: Vec::new(),
        }))
    }

    pub fn build_activity_with_phase(self, phase: impl Into<String>) -> Self {
        self.variant(ActivityVariant::Build(BuildActivity {
            phase: Some(phase.into()),
            log_stdout_lines: Vec::new(),
            log_stderr_lines: Vec::new(),
        }))
    }

    pub fn download_activity(self, current: Option<u64>, total: Option<u64>) -> Self {
        self.variant(ActivityVariant::Download(DownloadActivity {
            size_current: current,
            size_total: total,
            speed: None,
            substituter: None,
        }))
    }

    pub fn download_activity_with_substituter(
        self,
        current: Option<u64>,
        total: Option<u64>,
        substituter: impl Into<String>,
    ) -> Self {
        self.variant(ActivityVariant::Download(DownloadActivity {
            size_current: current,
            size_total: total,
            speed: None,
            substituter: Some(substituter.into()),
        }))
    }

    pub fn query_activity(self) -> Self {
        self.variant(ActivityVariant::Query(QueryActivity { substituter: None }))
    }

    pub fn query_activity_with_substituter(self, substituter: impl Into<String>) -> Self {
        self.variant(ActivityVariant::Query(QueryActivity {
            substituter: Some(substituter.into()),
        }))
    }

    pub fn task_activity(self, status: TaskDisplayStatus) -> Self {
        self.variant(ActivityVariant::Task(TaskActivity {
            status,
            duration: None,
        }))
    }

    pub fn task_activity_with_duration(
        self,
        status: TaskDisplayStatus,
        duration_secs: u64,
    ) -> Self {
        self.variant(ActivityVariant::Task(TaskActivity {
            status,
            duration: Some(Duration::from_secs(duration_secs)),
        }))
    }

    pub fn evaluating_activity(self) -> Self {
        self.variant(ActivityVariant::Evaluating)
    }

    pub fn fetch_tree_activity(self) -> Self {
        self.variant(ActivityVariant::FetchTree)
    }

    pub fn user_operation_activity(self) -> Self {
        self.variant(ActivityVariant::UserOperation)
    }
}

pub fn create_test_fields(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
