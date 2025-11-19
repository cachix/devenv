use devenv_tui::{LogLevel, LogMessage, LogSource, Model, TaskDisplayStatus};
use devenv_activity::{ActivityEvent, ActivityKind, ActivityOutcome, ProgressState, ProgressUnit};
use std::collections::HashMap;
use std::time::SystemTime;

#[test]
fn test_new_model_is_empty() {
    let model = Model::new();

    assert!(model.activities.is_empty());
    assert!(model.message_log.is_empty());
    assert!(model.root_activities.is_empty());
}

#[test]
fn test_add_activity_inserts_activity() {
    let mut model = Model::new();
    let activity_id = 1;

    let event = ActivityEvent::Start {
        id: activity_id,
        kind: ActivityKind::Build,
        name: "Building example-package".to_string(),
        parent: None,
        detail: Some("/nix/store/abc123-example-package.drv".to_string()),
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(event);

    assert_eq!(model.activities.len(), 1);
    assert!(model.activities.contains_key(&activity_id));
}

#[test]
fn test_add_log_message() {
    let mut model = Model::new();

    let log_msg = LogMessage::new(
        LogLevel::Info,
        "Test message".to_string(),
        LogSource::User,
        HashMap::new(),
    );

    model.add_log_message(log_msg);

    assert_eq!(model.message_log.len(), 1);
    assert_eq!(model.message_log[0].message, "Test message");
    assert_eq!(model.message_log[0].level, LogLevel::Info);
}

#[test]
fn test_get_active_activities() {
    let mut model = Model::new();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Build,
        name: "Building example-package".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(event);

    let active_count = model.get_active_activities().len();
    assert!(active_count > 0);
}

#[test]
fn test_model_with_hierarchy_structure() {
    let mut model = Model::new();
    let parent_id = 1;
    let child_ids = vec![2, 3, 4];

    // Create parent activity
    let parent_event = ActivityEvent::Start {
        id: parent_id,
        kind: ActivityKind::Operation,
        name: "Parent operation".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };
    model.apply_activity_event(parent_event);

    // Create child activities
    for &child_id in &child_ids {
        let child_event = ActivityEvent::Start {
            id: child_id,
            kind: ActivityKind::Build,
            name: format!("Child operation {}", child_id),
            parent: Some(parent_id),
            detail: None,
            timestamp: SystemTime::now(),
        };
        model.apply_activity_event(child_event);
    }

    assert!(model.activities.contains_key(&parent_id));
    assert_eq!(model.root_activities.len(), 1);
    assert_eq!(model.root_activities[0], parent_id);

    for child_id in &child_ids {
        assert!(model.activities.contains_key(child_id));
        assert_eq!(model.activities[child_id].parent_id, Some(parent_id));
    }
}

#[test]
fn test_start_event_creates_build_activity() {
    let mut model = Model::new();

    let event = ActivityEvent::Start {
        id: 42,
        kind: ActivityKind::Build,
        name: "Test Build".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(event);

    assert!(model.activities.contains_key(&42));
    let activity = &model.activities[&42];
    assert_eq!(activity.id, 42);
    assert_eq!(activity.name, "Test Build");
}

#[test]
fn test_progress_event_updates_download_activity() {
    let mut model = Model::new();

    let start_event = ActivityEvent::Start {
        id: 100,
        kind: ActivityKind::Fetch,
        name: "Download Package".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(start_event);

    let progress_event = ActivityEvent::Progress {
        id: 100,
        progress: ProgressState::Determinate {
            current: 1024,
            total: 2048,
            unit: Some(ProgressUnit::Bytes),
        },
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(progress_event);

    assert!(model.activities.contains_key(&100));
    let activity = &model.activities[&100];
    assert_eq!(activity.id, 100);
    assert!(activity.progress.is_some());
}

#[test]
fn test_complete_event_marks_task_as_completed() {
    let mut model = Model::new();

    let start_event = ActivityEvent::Start {
        id: 200,
        kind: ActivityKind::Task,
        name: "Run Tests".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(start_event);

    let complete_event = ActivityEvent::Complete {
        id: 200,
        outcome: ActivityOutcome::Success,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(complete_event);

    assert!(model.activities.contains_key(&200));
    let activity = &model.activities[&200];
    assert_eq!(activity.id, 200);
}

#[test]
fn test_multiple_activities_different_types() {
    let mut model = Model::new();

    let build_event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Build,
        name: "Build Package".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    let download_event = ActivityEvent::Start {
        id: 2,
        kind: ActivityKind::Fetch,
        name: "Download Package".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    let task_event = ActivityEvent::Start {
        id: 3,
        kind: ActivityKind::Task,
        name: "Run Task".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(build_event);
    model.apply_activity_event(download_event);
    model.apply_activity_event(task_event);

    assert_eq!(model.activities.len(), 3);
    assert!(model.activities.contains_key(&1));
    assert!(model.activities.contains_key(&2));
    assert!(model.activities.contains_key(&3));
}

#[test]
fn test_task_activity_running() {
    let mut model = Model::new();

    let event = ActivityEvent::Start {
        id: 4,
        kind: ActivityKind::Task,
        name: "Running test suite".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(event);

    let activity = &model.activities[&4];
    assert_eq!(activity.name, "Running test suite");
}

#[test]
fn test_activity_event_with_all_fields() {
    let mut model = Model::new();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Fetch,
        name: "Test Activity".to_string(),
        parent: Some(99),
        detail: Some("Some detail".to_string()),
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(event);

    let activity = &model.activities[&1];
    assert_eq!(activity.name, "Test Activity");
    assert_eq!(activity.parent_id, Some(99));
    assert_eq!(activity.detail, Some("Some detail".to_string()));
}

#[test]
fn test_calculate_summary() {
    let mut model = Model::new();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Build,
        name: "Building example-package".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(event);

    let summary = model.calculate_summary();
    assert!(summary.total_builds >= 1);
}

#[test]
fn test_select_next_build() {
    let mut model = Model::new();

    let build1_event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Build,
        name: "Build Package 1".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    let build2_event = ActivityEvent::Start {
        id: 2,
        kind: ActivityKind::Build,
        name: "Build Package 2".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(build1_event);
    model.apply_activity_event(build2_event);

    // Initially no selection
    assert!(model.ui.selected_activity.is_none());

    // Select next should select first build
    model.select_next_build();
    assert!(model.ui.selected_activity.is_some());
}

#[test]
fn test_get_display_activities() {
    let mut model = Model::new();
    let parent_id = 1;
    let child_ids = vec![2, 3, 4];

    let parent_event = ActivityEvent::Start {
        id: parent_id,
        kind: ActivityKind::Operation,
        name: "Parent operation".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };
    model.apply_activity_event(parent_event);

    for &child_id in &child_ids {
        let child_event = ActivityEvent::Start {
            id: child_id,
            kind: ActivityKind::Build,
            name: format!("Child operation {}", child_id),
            parent: Some(parent_id),
            detail: None,
            timestamp: SystemTime::now(),
        };
        model.apply_activity_event(child_event);
    }

    let display_activities = model.get_display_activities();

    // Should have parent and all children
    assert_eq!(display_activities.len(), 4);
}
