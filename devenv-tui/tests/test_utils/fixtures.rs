use devenv_tui::{
    Activity, ActivityVariant, BuildActivity, DownloadActivity, Model, NixActivityState,
    OperationId, TaskActivity, TaskDisplayStatus,
};
use std::time::{Duration, Instant};

pub fn simple_build_activity() -> Activity {
    Activity {
        id: 1,
        operation_id: OperationId::from_activity_id(1),
        name: "Building example-package".to_string(),
        short_name: "example-package".to_string(),
        parent_id: None,
        start_time: Instant::now(),
        state: NixActivityState::Active,
        detail: Some("/nix/store/abc123-example-package.drv".to_string()),
        variant: ActivityVariant::Build(BuildActivity {
            phase: Some("buildPhase".to_string()),
            log_stdout_lines: vec!["Building...".to_string()],
            log_stderr_lines: Vec::new(),
        }),
        progress: None,
    }
}

pub fn completed_download_activity() -> Activity {
    Activity {
        id: 2,
        operation_id: OperationId::from_activity_id(2),
        name: "Downloading nixpkgs".to_string(),
        short_name: "nixpkgs".to_string(),
        parent_id: None,
        start_time: Instant::now(),
        state: NixActivityState::Completed {
            success: true,
            duration: Duration::from_secs(5),
        },
        detail: Some("/nix/store/xyz789-nixpkgs".to_string()),
        variant: ActivityVariant::Download(DownloadActivity {
            size_current: Some(1024 * 1024 * 10),
            size_total: Some(1024 * 1024 * 10),
            speed: Some(1024 * 1024 * 2),
            substituter: Some("https://cache.nixos.org".to_string()),
        }),
        progress: None,
    }
}

pub fn failed_build_activity() -> Activity {
    Activity {
        id: 3,
        operation_id: OperationId::from_activity_id(3),
        name: "Building failed-package".to_string(),
        short_name: "failed-package".to_string(),
        parent_id: None,
        start_time: Instant::now(),
        state: NixActivityState::Completed {
            success: false,
            duration: Duration::from_secs(30),
        },
        detail: Some("/nix/store/fail123-failed-package.drv".to_string()),
        variant: ActivityVariant::Build(BuildActivity {
            phase: Some("buildPhase".to_string()),
            log_stdout_lines: Vec::new(),
            log_stderr_lines: vec![
                "error: builder for '/nix/store/fail123-failed-package.drv' failed".to_string(),
            ],
        }),
        progress: None,
    }
}

pub fn task_activity_running() -> Activity {
    Activity {
        id: 4,
        operation_id: OperationId::from_activity_id(4),
        name: "Running test suite".to_string(),
        short_name: "test".to_string(),
        parent_id: None,
        start_time: Instant::now(),
        state: NixActivityState::Active,
        detail: None,
        variant: ActivityVariant::Task(TaskActivity {
            status: TaskDisplayStatus::Running,
            duration: None,
        }),
        progress: None,
    }
}

pub fn model_with_activities() -> Model {
    let mut model = Model::new();

    let build = simple_build_activity();
    let download = completed_download_activity();
    let failed = failed_build_activity();

    model.root_activities.push(build.id);
    model.root_activities.push(download.id);
    model.root_activities.push(failed.id);

    model.activities.insert(build.id, build);
    model.activities.insert(download.id, download);
    model.activities.insert(failed.id, failed);

    model
}

pub fn model_with_hierarchy() -> (Model, u64, Vec<u64>) {
    let mut model = Model::new();

    let parent_id = 100;
    let child_ids = vec![101, 102, 103];

    // Create parent activity
    let parent = Activity {
        id: parent_id,
        operation_id: OperationId::from_activity_id(parent_id),
        name: "Parent activity".to_string(),
        short_name: "parent".to_string(),
        parent_id: None,
        start_time: Instant::now(),
        state: NixActivityState::Active,
        detail: None,
        variant: ActivityVariant::UserOperation,
        progress: None,
    };

    model.root_activities.push(parent_id);
    model.activities.insert(parent_id, parent);

    // Create child activities
    for child_id in &child_ids {
        let child = Activity {
            id: *child_id,
            operation_id: OperationId::from_activity_id(*child_id),
            name: format!("Child activity {}", child_id),
            short_name: format!("child-{}", child_id),
            parent_id: Some(parent_id),
            start_time: Instant::now(),
            state: NixActivityState::Active,
            detail: None,
            variant: ActivityVariant::Build(BuildActivity {
                phase: Some("buildPhase".to_string()),
                log_stdout_lines: Vec::new(),
                log_stderr_lines: Vec::new(),
            }),
            progress: None,
        };
        model.activities.insert(*child_id, child);
    }

    (model, parent_id, child_ids)
}
