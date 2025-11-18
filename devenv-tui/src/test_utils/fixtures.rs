//! Test fixtures for creating pre-configured test data

use crate::{Activity, ActivityVariant, BuildActivity, Model, TaskActivity, TaskDisplayStatus};
use devenv_activity::ActivityKind;
use std::time::Instant;

pub fn simple_build_activity() -> Activity {
    Activity {
        id: 1,
        name: "Building example-package".to_string(),
        short_name: "example-package".to_string(),
        parent: None,
        children: Vec::new(),
        start_time: Instant::now(),
        completed: None,
        detail: Some("/nix/store/abc123-example-package.drv".to_string()),
        kind: ActivityKind::Build,
        variant: ActivityVariant::Build(BuildActivity {
            phase: Some("buildPhase".to_string()),
        }),
        progress: None,
    }
}

pub fn task_activity_running() -> Activity {
    Activity {
        id: 4,
        name: "Running test suite".to_string(),
        short_name: "test".to_string(),
        parent: None,
        children: Vec::new(),
        start_time: Instant::now(),
        completed: None,
        detail: None,
        kind: ActivityKind::Task,
        variant: ActivityVariant::Task(TaskActivity {
            status: TaskDisplayStatus::Running,
        }),
        progress: None,
    }
}

pub fn model_with_activities() -> Model {
    let mut model = Model::new();

    let build = simple_build_activity();
    model.add_activity(build);

    model
}

pub fn model_with_hierarchy() -> Model {
    let mut model = Model::new();

    // Create parent activity
    let parent = Activity {
        id: 1,
        name: "Parent operation".to_string(),
        short_name: "parent".to_string(),
        parent: None,
        children: Vec::new(),
        start_time: Instant::now(),
        completed: None,
        detail: None,
        kind: ActivityKind::Operation,
        variant: ActivityVariant::UserOperation,
        progress: None,
    };
    model.add_activity(parent);

    // Create child activities
    for i in 2..=4 {
        let child = Activity {
            id: i,
            name: format!("Child operation {}", i),
            short_name: format!("child-{}", i),
            parent: Some(1),
            children: Vec::new(),
            start_time: Instant::now(),
            completed: None,
            detail: None,
            kind: ActivityKind::Build,
            variant: ActivityVariant::Build(BuildActivity { phase: None }),
            progress: None,
        };
        model.add_activity(child);
    }

    model
}
