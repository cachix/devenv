//! End-to-end TUI snapshot tests.
//!
//! These tests verify that when events are fed into the model,
//! the TUI renders the expected output.

use devenv_tui::{Model, TaskDisplayStatus};

use crate::test_utils::builders::ActivityBuilder;
use crate::test_utils::render::render_to_string;

/// Helper to add an activity to the model.
fn add_activity(model: &mut Model, activity: devenv_tui::Activity) {
    let id = activity.id;
    if activity.parent_id.is_none() {
        model.root_activities.push(id);
    }
    model.activities.insert(id, activity);
}

/// Test that an empty model renders correctly.
#[test]
fn test_empty_model() {
    let model = Model::new();
    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test that a single build activity shows in the TUI.
#[test]
fn test_single_build_activity() {
    let mut model = Model::new();

    let activity = ActivityBuilder::new(1)
        .name("Building hello-world")
        .short_name("hello-world")
        .build_activity_with_phase("buildPhase")
        .build();

    add_activity(&mut model, activity);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test that a download activity with progress shows in the TUI.
#[test]
fn test_download_with_progress() {
    let mut model = Model::new();

    let activity = ActivityBuilder::new(1)
        .name("Downloading nixpkgs")
        .short_name("nixpkgs")
        .download_activity(Some(5000), Some(10000))
        .build();

    add_activity(&mut model, activity);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test that a task activity shows in the TUI.
#[test]
fn test_task_running() {
    let mut model = Model::new();

    let activity = ActivityBuilder::new(1)
        .name("Running tests")
        .short_name("tests")
        .task_activity(TaskDisplayStatus::Running)
        .build();

    add_activity(&mut model, activity);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test that multiple concurrent activities show in the TUI.
#[test]
fn test_multiple_activities() {
    let mut model = Model::new();

    let build = ActivityBuilder::new(1)
        .name("Building package-a")
        .short_name("package-a")
        .build_activity_with_phase("buildPhase")
        .build();

    let download = ActivityBuilder::new(2)
        .name("Downloading package-b")
        .short_name("package-b")
        .download_activity(Some(2500), Some(5000))
        .build();

    let task = ActivityBuilder::new(3)
        .name("Running setup")
        .short_name("setup")
        .task_activity(TaskDisplayStatus::Running)
        .build();

    add_activity(&mut model, build);
    add_activity(&mut model, download);
    add_activity(&mut model, task);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test task status lifecycle: pending -> running -> success.
#[test]
fn test_task_success() {
    let mut model = Model::new();

    let activity = ActivityBuilder::new(1)
        .name("Build completed")
        .short_name("build")
        .task_activity(TaskDisplayStatus::Success)
        .build();

    add_activity(&mut model, activity);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test task failure shows in the TUI.
#[test]
fn test_task_failed() {
    let mut model = Model::new();

    let activity = ActivityBuilder::new(1)
        .name("Tests failed")
        .short_name("tests")
        .task_activity(TaskDisplayStatus::Failed)
        .build();

    add_activity(&mut model, activity);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test evaluating activity shows in the TUI.
#[test]
fn test_evaluating_activity() {
    let mut model = Model::new();

    let activity = ActivityBuilder::new(1)
        .name("Evaluating flake")
        .short_name("flake")
        .evaluating_activity()
        .build();

    add_activity(&mut model, activity);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test query activity shows in the TUI.
#[test]
fn test_query_activity() {
    let mut model = Model::new();

    let activity = ActivityBuilder::new(1)
        .name("Querying cache")
        .short_name("cache")
        .query_activity_with_substituter("https://cache.nixos.org")
        .build();

    add_activity(&mut model, activity);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test fetch tree activity shows in the TUI.
#[test]
fn test_fetch_tree_activity() {
    let mut model = Model::new();

    let activity = ActivityBuilder::new(1)
        .name("Fetching github:NixOS/nixpkgs")
        .short_name("nixpkgs")
        .fetch_tree_activity()
        .build();

    add_activity(&mut model, activity);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test download with substituter info shows in the TUI.
#[test]
fn test_download_with_substituter() {
    let mut model = Model::new();

    let activity = ActivityBuilder::new(1)
        .name("Downloading package")
        .short_name("package")
        .download_activity_with_substituter(Some(1000), Some(2000), "https://cache.nixos.org")
        .build();

    add_activity(&mut model, activity);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}
