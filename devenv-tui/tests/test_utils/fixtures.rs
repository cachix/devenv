use devenv_tui::{
    Activity, ActivityVariant, BuildActivity, DownloadActivity, Model, NixActivityState,
    Operation, OperationId, OperationState, TaskActivity, TaskDisplayStatus,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub fn simple_build_activity() -> Activity {
    Activity {
        id: 1,
        operation_id: OperationId::new("build-1"),
        name: "Building example-package".to_string(),
        short_name: "example-package".to_string(),
        parent_operation: None,
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
        operation_id: OperationId::new("download-1"),
        name: "Downloading nixpkgs".to_string(),
        short_name: "nixpkgs".to_string(),
        parent_operation: None,
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
        operation_id: OperationId::new("build-2"),
        name: "Building failed-package".to_string(),
        short_name: "failed-package".to_string(),
        parent_operation: None,
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
        operation_id: OperationId::new("task-1"),
        name: "Running test suite".to_string(),
        short_name: "test".to_string(),
        parent_operation: None,
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

pub fn simple_operation() -> Operation {
    Operation {
        id: OperationId::new("op-1"),
        message: "Test operation".to_string(),
        state: OperationState::Active,
        start_time: Instant::now(),
        children: Vec::new(),
        parent: None,
        data: HashMap::new(),
    }
}

pub fn completed_operation() -> Operation {
    Operation {
        id: OperationId::new("op-2"),
        message: "Completed operation".to_string(),
        state: OperationState::Complete {
            duration: Duration::from_secs(10),
            success: true,
        },
        start_time: Instant::now(),
        children: Vec::new(),
        parent: None,
        data: HashMap::new(),
    }
}

pub fn operation_with_children() -> (Operation, Vec<OperationId>) {
    let child_ids = vec![
        OperationId::new("child-1"),
        OperationId::new("child-2"),
        OperationId::new("child-3"),
    ];

    let parent = Operation {
        id: OperationId::new("parent-op"),
        message: "Parent operation".to_string(),
        state: OperationState::Active,
        start_time: Instant::now(),
        children: child_ids.clone(),
        parent: None,
        data: HashMap::new(),
    };

    (parent, child_ids)
}

pub fn model_with_activities() -> Model {
    let mut model = Model::new();

    let build = simple_build_activity();
    let download = completed_download_activity();
    let failed = failed_build_activity();

    model.activities.insert(build.id, build);
    model.activities.insert(download.id, download);
    model.activities.insert(failed.id, failed);

    model
}

pub fn model_with_operations() -> Model {
    let mut model = Model::new();

    let op1 = simple_operation();
    let op2 = completed_operation();

    model.operations.insert(op1.id.clone(), op1);
    model.operations.insert(op2.id.clone(), op2);

    model
}

pub fn model_with_hierarchy() -> (Model, OperationId, Vec<OperationId>) {
    let mut model = Model::new();

    let (parent_op, child_ids) = operation_with_children();
    let parent_id = parent_op.id.clone();

    model.operations.insert(parent_id.clone(), parent_op);
    model.root_operations.push(parent_id.clone());

    for child_id in &child_ids {
        let child = Operation {
            id: child_id.clone(),
            message: format!("Child operation {}", child_id),
            state: OperationState::Active,
            start_time: Instant::now(),
            children: Vec::new(),
            parent: Some(parent_id.clone()),
            data: HashMap::new(),
        };
        model.operations.insert(child_id.clone(), child);
    }

    (model, parent_id, child_ids)
}
