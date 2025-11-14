use devenv_tui::{
    ActivityVariant, DataEvent, LogLevel, LogMessage, LogSource,
    Model, NixActivityState, OperationId, OperationResult, OperationState,
    TaskDisplayStatus,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::test_utils::builders::{ActivityBuilder, OperationBuilder};
use crate::test_utils::fixtures::{
    completed_operation, model_with_activities, model_with_hierarchy, simple_build_activity,
    simple_operation, task_activity_running,
};

#[test]
fn test_new_model_is_empty() {
    let model = Model::new();

    assert!(model.operations.is_empty());
    assert!(model.activities.is_empty());
    assert!(model.message_log.is_empty());
    assert!(model.root_operations.is_empty());
}

#[test]
fn test_register_operation_adds_to_operations() {
    let mut model = Model::new();
    let op_id = OperationId::new("test-op");

    let event = DataEvent::RegisterOperation {
        operation_id: op_id.clone(),
        operation_name: "Test Operation".to_string(),
        parent: None,
        fields: HashMap::new(),
    };

    event.apply(&mut model);

    assert_eq!(model.operations.len(), 1);
    assert!(model.operations.contains_key(&op_id));
    assert_eq!(model.operations[&op_id].message, "Test Operation");
}

#[test]
fn test_register_operation_with_parent_updates_parent_children() {
    let mut model = Model::new();
    let parent_id = OperationId::new("parent");
    let child_id = OperationId::new("child");

    let parent_event = DataEvent::RegisterOperation {
        operation_id: parent_id.clone(),
        operation_name: "Parent".to_string(),
        parent: None,
        fields: HashMap::new(),
    };
    parent_event.apply(&mut model);

    let child_event = DataEvent::RegisterOperation {
        operation_id: child_id.clone(),
        operation_name: "Child".to_string(),
        parent: Some(parent_id.clone()),
        fields: HashMap::new(),
    };
    child_event.apply(&mut model);

    assert_eq!(model.operations.len(), 2);
    assert_eq!(model.operations[&parent_id].children.len(), 1);
    assert_eq!(model.operations[&parent_id].children[0], child_id);
    assert_eq!(model.operations[&child_id].parent, Some(parent_id));
}

#[test]
fn test_register_root_operation_adds_to_root_operations() {
    let mut model = Model::new();
    let op_id = OperationId::new("root-op");

    let event = DataEvent::RegisterOperation {
        operation_id: op_id.clone(),
        operation_name: "Root Operation".to_string(),
        parent: None,
        fields: HashMap::new(),
    };

    event.apply(&mut model);

    assert_eq!(model.root_operations.len(), 1);
    assert_eq!(model.root_operations[0], op_id);
}

#[test]
fn test_close_operation_marks_complete() {
    let mut model = Model::new();
    let op_id = OperationId::new("test-op");

    let register_event = DataEvent::RegisterOperation {
        operation_id: op_id.clone(),
        operation_name: "Test Op".to_string(),
        parent: None,
        fields: HashMap::new(),
    };
    register_event.apply(&mut model);

    let close_event = DataEvent::CloseOperation {
        operation_id: op_id.clone(),
        result: OperationResult::Success,
    };
    close_event.apply(&mut model);

    match &model.operations[&op_id].state {
        OperationState::Complete { success, .. } => {
            assert!(success);
        }
        _ => panic!("Operation should be complete"),
    }
}

#[test]
fn test_close_operation_with_failure() {
    let mut model = Model::new();
    let op_id = OperationId::new("test-op");

    let register_event = DataEvent::RegisterOperation {
        operation_id: op_id.clone(),
        operation_name: "Test Op".to_string(),
        parent: None,
        fields: HashMap::new(),
    };
    register_event.apply(&mut model);

    let close_event = DataEvent::CloseOperation {
        operation_id: op_id.clone(),
        result: OperationResult::Failure {
            message: "Build failed".to_string(),
            code: Some(1),
            output: None,
        },
    };
    close_event.apply(&mut model);

    match &model.operations[&op_id].state {
        OperationState::Complete { success, .. } => {
            assert!(!success);
        }
        _ => panic!("Operation should be complete"),
    }
}

#[test]
fn test_add_activity_inserts_activity() {
    let mut model = Model::new();
    let activity = simple_build_activity();
    let activity_id = activity.id;

    let event = DataEvent::AddActivity(activity);
    event.apply(&mut model);

    assert_eq!(model.activities.len(), 1);
    assert!(model.activities.contains_key(&activity_id));
}

#[test]
fn test_complete_activity_updates_state() {
    let mut model = Model::new();
    let activity = simple_build_activity();
    let activity_id = activity.id;

    let add_event = DataEvent::AddActivity(activity);
    add_event.apply(&mut model);

    let complete_event = DataEvent::CompleteActivity {
        activity_id,
        success: true,
        end_time: Instant::now(),
    };
    complete_event.apply(&mut model);

    match &model.activities[&activity_id].state {
        NixActivityState::Completed { success, .. } => {
            assert!(success);
        }
        _ => panic!("Activity should be completed"),
    }
}

#[test]
fn test_complete_activity_with_failure() {
    let mut model = Model::new();
    let activity = simple_build_activity();
    let activity_id = activity.id;

    let add_event = DataEvent::AddActivity(activity);
    add_event.apply(&mut model);

    let complete_event = DataEvent::CompleteActivity {
        activity_id,
        success: false,
        end_time: Instant::now(),
    };
    complete_event.apply(&mut model);

    match &model.activities[&activity_id].state {
        NixActivityState::Completed { success, .. } => {
            assert!(!success);
        }
        _ => panic!("Activity should be completed"),
    }
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

    let event = DataEvent::AddLogMessage(log_msg);
    event.apply(&mut model);

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

    assert!(model.operations.contains_key(&parent_id));
    assert_eq!(model.root_operations.len(), 1);
    assert_eq!(model.root_operations[0], parent_id);

    for child_id in &child_ids {
        assert!(model.operations.contains_key(child_id));
        assert_eq!(model.operations[child_id].parent, Some(parent_id.clone()));
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
fn test_operation_builder_creates_valid_operation() {
    let op = OperationBuilder::new("test-op")
        .message("Building package")
        .data("key1", "value1")
        .data("key2", "value2")
        .build();

    assert_eq!(op.id, OperationId::new("test-op"));
    assert_eq!(op.message, "Building package");
    assert_eq!(op.data.get("key1"), Some(&"value1".to_string()));
    assert_eq!(op.data.get("key2"), Some(&"value2".to_string()));
}

#[test]
fn test_operation_builder_with_completion() {
    let op = OperationBuilder::new("completed-op")
        .message("Completed task")
        .completed(true, 10)
        .build();

    match op.state {
        OperationState::Complete { success, duration } => {
            assert!(success);
            assert_eq!(duration, Duration::from_secs(10));
        }
        _ => panic!("Expected Complete state"),
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

    DataEvent::AddActivity(build).apply(&mut model);
    DataEvent::AddActivity(download).apply(&mut model);
    DataEvent::AddActivity(task).apply(&mut model);

    assert_eq!(model.activities.len(), 3);

    assert_matches::assert_matches!(
        model.activities[&1].variant,
        ActivityVariant::Build(_)
    );
    assert_matches::assert_matches!(
        model.activities[&2].variant,
        ActivityVariant::Download(_)
    );
    assert_matches::assert_matches!(
        model.activities[&3].variant,
        ActivityVariant::Task(_)
    );
}

#[test]
fn test_remove_build_logs() {
    let mut model = Model::new();
    let activity_id = 1;

    let activity = ActivityBuilder::new(activity_id)
        .build_activity()
        .build();

    DataEvent::AddActivity(activity).apply(&mut model);

    model.build_logs.insert(activity_id, vec!["log line 1".to_string()].into());

    assert!(model.build_logs.contains_key(&activity_id));

    let remove_event = DataEvent::RemoveBuildLogs { activity_id };
    remove_event.apply(&mut model);

    assert!(!model.build_logs.contains_key(&activity_id));
}

#[test]
fn test_simple_operation_fixture() {
    let operation = simple_operation();

    assert_eq!(operation.id.0, "op-1");
    assert_eq!(operation.message, "Test operation");
    match operation.state {
        OperationState::Active => (),
        _ => panic!("Expected Active state"),
    }
    assert!(operation.children.is_empty());
    assert!(operation.parent.is_none());
}

#[test]
fn test_completed_operation_fixture() {
    let operation = completed_operation();

    assert_eq!(operation.id.0, "op-2");
    assert_eq!(operation.message, "Completed operation");
    match operation.state {
        OperationState::Complete { success, .. } => {
            assert!(success);
        }
        _ => panic!("Expected Complete state"),
    }
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
fn test_operation_builder_with_parent_child() {
    let parent = OperationBuilder::new("parent-op")
        .message("Parent")
        .build();

    let parent_id = parent.id.clone();
    let child = OperationBuilder::new("child-op")
        .message("Child")
        .parent(parent_id.clone())
        .build();

    assert_eq!(child.parent, Some(parent_id.clone()));
    assert_eq!(parent.id.0, "parent-op");
    assert_eq!(child.id.0, "child-op");
}

#[test]
fn test_activity_builder_with_all_fields() {
    let activity = ActivityBuilder::new(1)
        .name("Test Activity")
        .short_name("test")
        .operation_id(OperationId::new("op-1"))
        .parent_operation(OperationId::new("parent-op"))
        .detail("Some detail")
        .download_activity(Some(100), Some(200))
        .build();

    assert_eq!(activity.name, "Test Activity");
    assert_eq!(activity.short_name, "test");
    assert_eq!(activity.operation_id.0, "op-1");
    assert_eq!(activity.parent_operation, Some(OperationId::new("parent-op")));
    assert_eq!(activity.detail, Some("Some detail".to_string()));
}
