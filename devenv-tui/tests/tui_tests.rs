//! TUI snapshot tests.
//!
//! These tests verify that when activity events are fed into the model,
//! the TUI renders the expected output.

use devenv_activity::{ActivityEvent, ActivityKind, ProgressState, ProgressUnit, Timestamp};
use devenv_tui::{view::view, Model};
use iocraft::prelude::*;

const TEST_WIDTH: u16 = 80;
const TEST_HEIGHT: u16 = 24;

fn render_to_string(model: &Model) -> String {
    let mut element = view(model).into();
    element.render(Some(TEST_WIDTH as usize)).to_string()
}

fn new_test_model() -> Model {
    Model::with_terminal_size(TEST_WIDTH, TEST_HEIGHT)
}

/// Test that an empty model renders correctly.
#[test]
fn test_empty_model() {
    let model = new_test_model();
    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test that a single build activity shows in the TUI.
#[test]
fn test_single_build_activity() {
    let mut model = new_test_model();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Build,
        name: "Building hello-world".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };

    model.apply_activity_event(event);

    let phase_event = ActivityEvent::Phase {
        id: 1,
        phase: "buildPhase".to_string(),
        timestamp: Timestamp::now(),
    };

    model.apply_activity_event(phase_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test that a download activity with progress shows in the TUI.
#[test]
fn test_download_with_progress() {
    let mut model = new_test_model();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Fetch,
        name: "Downloading nixpkgs".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };

    model.apply_activity_event(event);

    let progress_event = ActivityEvent::Progress {
        id: 1,
        progress: ProgressState::Determinate {
            current: 5000,
            total: 10000,
            unit: Some(ProgressUnit::Bytes),
        },
        timestamp: Timestamp::now(),
    };

    model.apply_activity_event(progress_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test that a task activity shows in the TUI.
#[test]
fn test_task_running() {
    let mut model = new_test_model();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Task,
        name: "Running tests".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };

    model.apply_activity_event(event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test that multiple concurrent activities show in the TUI.
#[test]
fn test_multiple_activities() {
    let mut model = new_test_model();

    let build_event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Build,
        name: "Building package-a".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };

    let build_phase_event = ActivityEvent::Phase {
        id: 1,
        phase: "buildPhase".to_string(),
        timestamp: Timestamp::now(),
    };

    let download_event = ActivityEvent::Start {
        id: 2,
        kind: ActivityKind::Fetch,
        name: "Downloading package-b".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };

    let download_progress_event = ActivityEvent::Progress {
        id: 2,
        progress: ProgressState::Determinate {
            current: 2500,
            total: 5000,
            unit: Some(ProgressUnit::Bytes),
        },
        timestamp: Timestamp::now(),
    };

    let task_event = ActivityEvent::Start {
        id: 3,
        kind: ActivityKind::Task,
        name: "Running setup".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };

    model.apply_activity_event(build_event);
    model.apply_activity_event(build_phase_event);
    model.apply_activity_event(download_event);
    model.apply_activity_event(download_progress_event);
    model.apply_activity_event(task_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test task status lifecycle: pending -> running -> success.
#[test]
fn test_task_success() {
    let mut model = new_test_model();

    let start_event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Task,
        name: "Build completed".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };

    model.apply_activity_event(start_event);

    let complete_event = ActivityEvent::Complete {
        id: 1,
        outcome: devenv_activity::ActivityOutcome::Success,
        timestamp: Timestamp::now(),
    };

    model.apply_activity_event(complete_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test task failure shows in the TUI.
#[test]
fn test_task_failed() {
    let mut model = new_test_model();

    let start_event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Task,
        name: "Tests failed".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };

    model.apply_activity_event(start_event);

    let complete_event = ActivityEvent::Complete {
        id: 1,
        outcome: devenv_activity::ActivityOutcome::Failed,
        timestamp: Timestamp::now(),
    };

    model.apply_activity_event(complete_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test evaluating activity shows in the TUI.
#[test]
fn test_evaluating_activity() {
    let mut model = new_test_model();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Evaluate,
        name: "Evaluating flake".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };

    model.apply_activity_event(event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test query activity shows in the TUI.
#[test]
fn test_query_activity() {
    let mut model = new_test_model();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Operation,
        name: "Querying cache".to_string(),
        parent: None,
        detail: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    };

    model.apply_activity_event(event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test fetch tree activity shows in the TUI.
#[test]
fn test_fetch_tree_activity() {
    let mut model = new_test_model();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Fetch,
        name: "Fetching github:NixOS/nixpkgs".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };

    model.apply_activity_event(event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test download with substituter info shows in the TUI.
#[test]
fn test_download_with_substituter() {
    let mut model = new_test_model();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Fetch,
        name: "Downloading package".to_string(),
        parent: None,
        detail: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    };

    model.apply_activity_event(event);

    let progress_event = ActivityEvent::Progress {
        id: 1,
        progress: ProgressState::Determinate {
            current: 1000,
            total: 2000,
            unit: Some(ProgressUnit::Bytes),
        },
        timestamp: Timestamp::now(),
    };

    model.apply_activity_event(progress_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}
