use devenv_tui::{Model, TaskDisplayStatus};

use crate::test_utils::builders::ActivityBuilder;
use crate::test_utils::fixtures::{model_with_activities, model_with_hierarchy};

fn format_model_summary(model: &Model) -> String {
    let mut lines = vec![];

    lines.push(format!("Operations: {}", model.operations.len()));
    lines.push(format!("Root operations: {}", model.root_operations.len()));
    lines.push(format!("Activities: {}", model.activities.len()));
    lines.push(format!("Active activities: {}", model.get_active_activities().len()));
    lines.push(format!("Log messages: {}", model.message_log.len()));
    lines.push(format!("Build logs: {}", model.build_logs.len()));

    lines.push(String::new());
    lines.push("Activity IDs:".to_string());
    let mut activity_ids: Vec<_> = model.activities.keys().collect();
    activity_ids.sort();
    for id in activity_ids {
        lines.push(format!("  - {}", id));
    }

    lines.join("\n")
}

#[test]
fn test_empty_model_snapshot() {
    let model = Model::new();
    let summary = format_model_summary(&model);

    insta::assert_snapshot!(summary, @r###"
    Operations: 0
    Root operations: 0
    Activities: 0
    Active activities: 0
    Log messages: 0
    Build logs: 0

    Activity IDs:
    "###);
}

#[test]
fn test_model_with_activities_snapshot() {
    let model = model_with_activities();
    let summary = format_model_summary(&model);

    insta::assert_snapshot!(summary, @r###"
    Operations: 0
    Root operations: 0
    Activities: 3
    Active activities: 1
    Log messages: 0
    Build logs: 0

    Activity IDs:
      - 1
      - 2
      - 3
    "###);
}

#[test]
fn test_model_with_hierarchy_snapshot() {
    let (model, _parent_id, _child_ids) = model_with_hierarchy();
    let summary = format_model_summary(&model);

    insta::assert_snapshot!(summary, @r###"
    Operations: 4
    Root operations: 1
    Activities: 0
    Active activities: 0
    Log messages: 0
    Build logs: 0

    Activity IDs:
    "###);
}

fn format_activity_list(model: &Model) -> String {
    let mut lines = vec![];

    let mut activities: Vec<_> = model.activities.values().collect();
    activities.sort_by_key(|a| a.id);

    for activity in activities {
        let state = match &activity.state {
            devenv_tui::NixActivityState::Active => "Active",
            devenv_tui::NixActivityState::Completed { success, .. } => {
                if *success {
                    "Success"
                } else {
                    "Failed"
                }
            }
        };

        lines.push(format!(
            "[{}] {} - {} ({})",
            activity.id, activity.short_name, state, activity.name
        ));
    }

    if lines.is_empty() {
        lines.push("No activities".to_string());
    }

    lines.join("\n")
}

#[test]
fn test_activity_list_snapshot() {
    let model = model_with_activities();
    let list = format_activity_list(&model);

    insta::assert_snapshot!(list, @r###"
    [1] example-package - Active (Building example-package)
    [2] nixpkgs - Success (Downloading nixpkgs)
    [3] failed-package - Failed (Building failed-package)
    "###);
}

#[test]
fn test_mixed_activity_types_snapshot() {
    let mut model = Model::new();

    let build = ActivityBuilder::new(1)
        .name("Building rust-package")
        .short_name("rust-pkg")
        .build_activity_with_phase("buildPhase")
        .build();

    let download = ActivityBuilder::new(2)
        .name("Downloading nixpkgs")
        .short_name("nixpkgs")
        .download_activity(Some(5000), Some(10000))
        .build();

    let task = ActivityBuilder::new(3)
        .name("Running tests")
        .short_name("test")
        .task_activity(TaskDisplayStatus::Running)
        .build();

    let query = ActivityBuilder::new(4)
        .name("Querying cache")
        .short_name("query")
        .query_activity()
        .build();

    let eval = ActivityBuilder::new(5)
        .name("Evaluating Nix")
        .short_name("eval")
        .evaluating_activity()
        .build();

    devenv_tui::DataEvent::AddActivity(build).apply(&mut model);
    devenv_tui::DataEvent::AddActivity(download).apply(&mut model);
    devenv_tui::DataEvent::AddActivity(task).apply(&mut model);
    devenv_tui::DataEvent::AddActivity(query).apply(&mut model);
    devenv_tui::DataEvent::AddActivity(eval).apply(&mut model);

    let list = format_activity_list(&model);

    insta::assert_snapshot!(list, @r###"
    [1] rust-pkg - Active (Building rust-package)
    [2] nixpkgs - Active (Downloading nixpkgs)
    [3] test - Active (Running tests)
    [4] query - Active (Querying cache)
    [5] eval - Active (Evaluating Nix)
    "###);
}

fn format_operation_tree(model: &Model) -> String {
    let mut lines = vec![];

    fn format_operation_recursive(
        model: &Model,
        op_id: &devenv_tui::OperationId,
        depth: usize,
        lines: &mut Vec<String>,
    ) {
        if let Some(op) = model.operations.get(op_id) {
            let indent = "  ".repeat(depth);
            let state = match &op.state {
                devenv_tui::OperationState::Active => "Active",
                devenv_tui::OperationState::Complete { success, .. } => {
                    if *success {
                        "Success"
                    } else {
                        "Failed"
                    }
                }
            };

            lines.push(format!("{}├─ {} ({}) - {}", indent, op.id, state, op.message));

            for child_id in &op.children {
                format_operation_recursive(model, child_id, depth + 1, lines);
            }
        }
    }

    for root_id in &model.root_operations {
        format_operation_recursive(model, root_id, 0, &mut lines);
    }

    if lines.is_empty() {
        lines.push("No operations".to_string());
    }

    lines.join("\n")
}

#[test]
fn test_operation_tree_snapshot() {
    let (model, _parent_id, _child_ids) = model_with_hierarchy();
    let tree = format_operation_tree(&model);

    insta::assert_snapshot!(tree, @r###"
    ├─ parent-op (Active) - Parent operation
      ├─ child-1 (Active) - Child operation child-1
      ├─ child-2 (Active) - Child operation child-2
      ├─ child-3 (Active) - Child operation child-3
    "###);
}

fn format_spinner_frames() -> String {
    let frames = devenv_tui::components::SPINNER_FRAMES;
    frames.join(" ")
}

#[test]
fn test_spinner_frames_snapshot() {
    let frames = format_spinner_frames();
    insta::assert_snapshot!(frames, @"⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏");
}

#[test]
fn test_ui_state_defaults() {
    let model = Model::new();
    let ui = &model.ui;

    let summary = format!(
        "Spinner frame: {}\nViewport: min={}, max={}, current={}, visible={}\nSelected activity: {:?}\nScroll: activity_pos={}, log_offset={}\nShow details: {}\nShow expanded logs: {}",
        ui.spinner_frame,
        ui.viewport.min,
        ui.viewport.max,
        ui.viewport.current,
        ui.viewport.activities_visible,
        ui.selected_activity,
        ui.scroll.activity_position,
        ui.scroll.log_offset,
        ui.view_options.show_details,
        ui.view_options.show_expanded_logs,
    );

    insta::assert_snapshot!(summary, @r###"
    Spinner frame: 0
    Viewport: min=10, max=40, current=10, visible=5
    Selected activity: None
    Scroll: activity_pos=0, log_offset=0
    Show details: false
    Show expanded logs: false
    "###);
}
