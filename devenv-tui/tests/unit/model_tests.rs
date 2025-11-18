use devenv_tui::{
    ActivityVariant, LogLevel, LogMessage, LogSource, Model, NixActivityState, OperationId,
    TaskDisplayStatus,
};
use std::collections::HashMap;
use std::time::Duration;

use crate::test_utils::builders::ActivityBuilder;
use crate::test_utils::fixtures::{
    model_with_activities, model_with_hierarchy, simple_build_activity, task_activity_running,
};

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
    let activity = simple_build_activity();
    let activity_id = activity.id;

    model.root_activities.push(activity_id);
    model.activities.insert(activity_id, activity);

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
    let model = model_with_activities();

    let active_count = model.get_active_activities().len();

    assert!(active_count > 0);
}

#[test]
fn test_model_with_hierarchy_structure() {
    let (model, parent_id, child_ids) = model_with_hierarchy();

    assert!(model.activities.contains_key(&parent_id));
    assert_eq!(model.root_activities.len(), 1);
    assert_eq!(model.root_activities[0], parent_id);

    for child_id in &child_ids {
        assert!(model.activities.contains_key(child_id));
        assert_eq!(model.activities[child_id].parent_id, Some(parent_id));
    }
}

#[test]
fn test_builder_creates_valid_activity() {
    let activity = ActivityBuilder::new(42)
        .name("Test Build")
        .short_name("test")
        .build_activity_with_phase("buildPhase")
        .build();

    assert_eq!(activity.id, 42);
    assert_eq!(activity.name, "Test Build");
    assert_eq!(activity.short_name, "test");

    match activity.variant {
        ActivityVariant::Build(build) => {
            assert_eq!(build.phase, Some("buildPhase".to_string()));
        }
        _ => panic!("Expected Build variant"),
    }
}

#[test]
fn test_builder_creates_download_activity() {
    let activity = ActivityBuilder::new(100)
        .name("Download Package")
        .download_activity(Some(1024), Some(2048))
        .progress(1024, 2048, "bytes")
        .build();

    assert_eq!(activity.id, 100);

    match activity.variant {
        ActivityVariant::Download(download) => {
            assert_eq!(download.size_current, Some(1024));
            assert_eq!(download.size_total, Some(2048));
        }
        _ => panic!("Expected Download variant"),
    }

    assert!(activity.progress.is_some());
    let progress = activity.progress.unwrap();
    assert_eq!(progress.current, Some(1024));
    assert_eq!(progress.total, Some(2048));
    assert!(progress.percent.unwrap() > 49.0 && progress.percent.unwrap() < 51.0);
}

#[test]
fn test_builder_creates_task_activity() {
    let activity = ActivityBuilder::new(200)
        .name("Run Tests")
        .task_activity_with_duration(TaskDisplayStatus::Success, 30)
        .completed(true, 30)
        .build();

    assert_eq!(activity.id, 200);

    match activity.variant {
        ActivityVariant::Task(task) => {
            assert_eq!(task.status, TaskDisplayStatus::Success);
            assert_eq!(task.duration, Some(Duration::from_secs(30)));
        }
        _ => panic!("Expected Task variant"),
    }

    match activity.state {
        NixActivityState::Completed { success, duration } => {
            assert!(success);
            assert_eq!(duration, Duration::from_secs(30));
        }
        _ => panic!("Expected Completed state"),
    }
}

#[test]
fn test_multiple_activities_different_types() {
    let mut model = Model::new();

    let build = ActivityBuilder::new(1)
        .build_activity_with_phase("configurePhase")
        .build();

    let download = ActivityBuilder::new(2)
        .download_activity(Some(500), Some(1000))
        .build();

    let task = ActivityBuilder::new(3)
        .task_activity(TaskDisplayStatus::Running)
        .build();

    model.root_activities.push(1);
    model.root_activities.push(2);
    model.root_activities.push(3);

    model.activities.insert(1, build);
    model.activities.insert(2, download);
    model.activities.insert(3, task);

    assert_eq!(model.activities.len(), 3);

    assert_matches::assert_matches!(model.activities[&1].variant, ActivityVariant::Build(_));
    assert_matches::assert_matches!(model.activities[&2].variant, ActivityVariant::Download(_));
    assert_matches::assert_matches!(model.activities[&3].variant, ActivityVariant::Task(_));
}

#[test]
fn test_task_activity_running_fixture() {
    let activity = task_activity_running();

    assert_eq!(activity.name, "Running test suite");
    assert_eq!(activity.short_name, "test");
    assert_eq!(activity.state, NixActivityState::Active);
    assert!(matches!(activity.variant, ActivityVariant::Task(_)));
}

#[test]
fn test_activity_builder_with_all_fields() {
    let activity = ActivityBuilder::new(1)
        .name("Test Activity")
        .short_name("test")
        .operation_id(OperationId::new("op-1"))
        .parent_id(99)
        .detail("Some detail")
        .download_activity(Some(100), Some(200))
        .build();

    assert_eq!(activity.name, "Test Activity");
    assert_eq!(activity.short_name, "test");
    assert_eq!(activity.operation_id.0, "op-1");
    assert_eq!(activity.parent_id, Some(99));
    assert_eq!(activity.detail, Some("Some detail".to_string()));
}

#[test]
fn test_calculate_summary() {
    let model = model_with_activities();
    let summary = model.calculate_summary();

    // model_with_activities has 1 active build, 1 completed download, 1 failed build
    assert!(summary.total_builds >= 1);
    assert!(summary.completed_downloads >= 0);
}

#[test]
fn test_select_next_build() {
    let mut model = Model::new();

    // Add two build activities
    let build1 = ActivityBuilder::new(1).build_activity().build();
    let build2 = ActivityBuilder::new(2).build_activity().build();

    model.root_activities.push(1);
    model.root_activities.push(2);
    model.activities.insert(1, build1);
    model.activities.insert(2, build2);

    // Initially no selection
    assert!(model.ui.selected_activity.is_none());

    // Select next should select first build
    model.select_next_build();
    assert!(model.ui.selected_activity.is_some());
}

#[test]
fn test_get_display_activities() {
    let (model, _parent_id, _child_ids) = model_with_hierarchy();

    let display_activities = model.get_display_activities();

    // Should have parent and all children
    assert_eq!(display_activities.len(), 4);
}
