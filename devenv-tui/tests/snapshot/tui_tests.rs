//! End-to-end TUI snapshot tests.
//!
//! These tests verify that when events are fed into the model,
//! the TUI renders the expected output.

use devenv_tui::Model;
use devenv_activity::{ActivityEvent, ActivityKind, ProgressState, ProgressUnit};
use std::time::SystemTime;

use crate::test_utils::render::render_to_string;

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

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Build,
        name: "Building hello-world".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(event);

    let phase_event = ActivityEvent::Phase {
        id: 1,
        phase: "buildPhase".to_string(),
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(phase_event);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test that a download activity with progress shows in the TUI.
#[test]
fn test_download_with_progress() {
    let mut model = Model::new();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Fetch,
        name: "Downloading nixpkgs".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(event);

    let progress_event = ActivityEvent::Progress {
        id: 1,
        progress: ProgressState::Determinate {
            current: 5000,
            total: 10000,
            unit: Some(ProgressUnit::Bytes),
        },
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(progress_event);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test that a task activity shows in the TUI.
#[test]
fn test_task_running() {
    let mut model = Model::new();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Task,
        name: "Running tests".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(event);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test that multiple concurrent activities show in the TUI.
#[test]
fn test_multiple_activities() {
    let mut model = Model::new();

    let build_event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Build,
        name: "Building package-a".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    let build_phase_event = ActivityEvent::Phase {
        id: 1,
        phase: "buildPhase".to_string(),
        timestamp: SystemTime::now(),
    };

    let download_event = ActivityEvent::Start {
        id: 2,
        kind: ActivityKind::Fetch,
        name: "Downloading package-b".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    let download_progress_event = ActivityEvent::Progress {
        id: 2,
        progress: ProgressState::Determinate {
            current: 2500,
            total: 5000,
            unit: Some(ProgressUnit::Bytes),
        },
        timestamp: SystemTime::now(),
    };

    let task_event = ActivityEvent::Start {
        id: 3,
        kind: ActivityKind::Task,
        name: "Running setup".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(build_event);
    model.apply_activity_event(build_phase_event);
    model.apply_activity_event(download_event);
    model.apply_activity_event(download_progress_event);
    model.apply_activity_event(task_event);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test task status lifecycle: pending -> running -> success.
#[test]
fn test_task_success() {
    let mut model = Model::new();

    let start_event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Task,
        name: "Build completed".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(start_event);

    let complete_event = ActivityEvent::Complete {
        id: 1,
        outcome: devenv_activity::ActivityOutcome::Success,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(complete_event);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test task failure shows in the TUI.
#[test]
fn test_task_failed() {
    let mut model = Model::new();

    let start_event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Task,
        name: "Tests failed".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(start_event);

    let complete_event = ActivityEvent::Complete {
        id: 1,
        outcome: devenv_activity::ActivityOutcome::Failed,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(complete_event);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test evaluating activity shows in the TUI.
#[test]
fn test_evaluating_activity() {
    let mut model = Model::new();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Evaluate,
        name: "Evaluating flake".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(event);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test query activity shows in the TUI.
#[test]
fn test_query_activity() {
    let mut model = Model::new();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Operation,
        name: "Querying cache".to_string(),
        parent: None,
        detail: Some("https://cache.nixos.org".to_string()),
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(event);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test fetch tree activity shows in the TUI.
#[test]
fn test_fetch_tree_activity() {
    let mut model = Model::new();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Fetch,
        name: "Fetching github:NixOS/nixpkgs".to_string(),
        parent: None,
        detail: None,
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(event);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}

/// Test download with substituter info shows in the TUI.
#[test]
fn test_download_with_substituter() {
    let mut model = Model::new();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Fetch,
        name: "Downloading package".to_string(),
        parent: None,
        detail: Some("https://cache.nixos.org".to_string()),
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(event);

    let progress_event = ActivityEvent::Progress {
        id: 1,
        progress: ProgressState::Determinate {
            current: 1000,
            total: 2000,
            unit: Some(ProgressUnit::Bytes),
        },
        timestamp: SystemTime::now(),
    };

    model.apply_activity_event(progress_event);

    let output = render_to_string(&model, 80);
    insta::assert_snapshot!(output);
}
