//! TUI snapshot tests.
//!
//! These tests verify that when activity events are fed into the model,
//! the TUI renders the expected output.

use devenv_activity::{ActivityEvent, ActivityKind, ProgressState, ProgressUnit, Timestamp};
use devenv_tui::{Model, view::view};
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

/// Test that Nix evaluation with nested child activities (builds, fetches, downloads) shows hierarchy.
#[test]
fn test_nested_evaluation_with_children() {
    let mut model = new_test_model();

    // Parent: Nix evaluation
    let eval_event = ActivityEvent::Start {
        id: 100,
        kind: ActivityKind::Evaluate,
        name: "devenv.nix".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(eval_event);

    // Child: Fetch triggered during evaluation
    let fetch_event = ActivityEvent::Start {
        id: 101,
        kind: ActivityKind::Fetch,
        name: "github:NixOS/nixpkgs".to_string(),
        parent: Some(100),
        detail: None,
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(fetch_event);

    // Child: Build triggered during evaluation
    let build_event = ActivityEvent::Start {
        id: 102,
        kind: ActivityKind::Build,
        name: "hello-2.12".to_string(),
        parent: Some(100),
        detail: None,
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(build_event);

    let build_phase = ActivityEvent::Phase {
        id: 102,
        phase: "buildPhase".to_string(),
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(build_phase);

    // Child: Download triggered during evaluation
    let download_event = ActivityEvent::Start {
        id: 103,
        kind: ActivityKind::Fetch,
        name: "openssl-3.0.0".to_string(),
        parent: Some(100),
        detail: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(download_event);

    // Grandchild: Download triggered during build (nested 2 levels)
    let nested_download_event = ActivityEvent::Start {
        id: 104,
        kind: ActivityKind::Fetch,
        name: "glibc-2.35".to_string(),
        parent: Some(102),
        detail: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(nested_download_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test that activity details can be added and are stored correctly.
#[test]
fn test_activity_with_details() {
    use devenv_activity::ActivityEvent;
    let mut model = new_test_model();

    // Create a parent activity
    let parent_event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Operation,
        name: "Building shell".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(parent_event);

    // Add a detail to the activity
    let detail_event = ActivityEvent::Detail {
        id: 1,
        key: "command".to_string(),
        value: "nix eval --json .#devenv.config".to_string(),
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(detail_event);

    // Add another detail
    let detail_event2 = ActivityEvent::Detail {
        id: 1,
        key: "args".to_string(),
        value: "--warn-dirty false".to_string(),
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(detail_event2);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test multiple parallel builds running concurrently.
#[test]
fn test_multiple_parallel_builds() {
    let mut model = new_test_model();

    // Start multiple builds at different phases
    let build1 = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Build,
        name: "hello-2.12".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(build1);
    model.apply_activity_event(ActivityEvent::Phase {
        id: 1,
        phase: "buildPhase".to_string(),
        timestamp: Timestamp::now(),
    });

    let build2 = ActivityEvent::Start {
        id: 2,
        kind: ActivityKind::Build,
        name: "openssl-3.0.0".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(build2);
    model.apply_activity_event(ActivityEvent::Phase {
        id: 2,
        phase: "configurePhase".to_string(),
        timestamp: Timestamp::now(),
    });

    let build3 = ActivityEvent::Start {
        id: 3,
        kind: ActivityKind::Build,
        name: "python-3.11.5".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(build3);
    model.apply_activity_event(ActivityEvent::Phase {
        id: 3,
        phase: "installPhase".to_string(),
        timestamp: Timestamp::now(),
    });

    let build4 = ActivityEvent::Start {
        id: 4,
        kind: ActivityKind::Build,
        name: "gcc-12.3.0".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(build4);
    model.apply_activity_event(ActivityEvent::Phase {
        id: 4,
        phase: "unpackPhase".to_string(),
        timestamp: Timestamp::now(),
    });

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test parallel downloads and builds happening simultaneously.
#[test]
fn test_parallel_downloads_and_builds() {
    let mut model = new_test_model();

    // Two builds running
    let build1 = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Build,
        name: "hello-2.12".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(build1);
    model.apply_activity_event(ActivityEvent::Phase {
        id: 1,
        phase: "buildPhase".to_string(),
        timestamp: Timestamp::now(),
    });

    let build2 = ActivityEvent::Start {
        id: 2,
        kind: ActivityKind::Build,
        name: "curl-8.1.0".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(build2);
    model.apply_activity_event(ActivityEvent::Phase {
        id: 2,
        phase: "configurePhase".to_string(),
        timestamp: Timestamp::now(),
    });

    // Three downloads in progress
    let download1 = ActivityEvent::Start {
        id: 3,
        kind: ActivityKind::Fetch,
        name: "openssl-3.0.0".to_string(),
        parent: None,
        detail: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(download1);
    model.apply_activity_event(ActivityEvent::Progress {
        id: 3,
        progress: ProgressState::Determinate {
            current: 15_000_000,
            total: 30_000_000,
            unit: Some(ProgressUnit::Bytes),
        },
        timestamp: Timestamp::now(),
    });

    let download2 = ActivityEvent::Start {
        id: 4,
        kind: ActivityKind::Fetch,
        name: "glibc-2.37".to_string(),
        parent: None,
        detail: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(download2);
    model.apply_activity_event(ActivityEvent::Progress {
        id: 4,
        progress: ProgressState::Determinate {
            current: 8_000_000,
            total: 10_000_000,
            unit: Some(ProgressUnit::Bytes),
        },
        timestamp: Timestamp::now(),
    });

    let download3 = ActivityEvent::Start {
        id: 5,
        kind: ActivityKind::Fetch,
        name: "python-3.11.5".to_string(),
        parent: None,
        detail: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(download3);
    model.apply_activity_event(ActivityEvent::Progress {
        id: 5,
        progress: ProgressState::Determinate {
            current: 1_000_000,
            total: 50_000_000,
            unit: Some(ProgressUnit::Bytes),
        },
        timestamp: Timestamp::now(),
    });

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test indeterminate progress shows in the TUI.
#[test]
fn test_indeterminate_progress() {
    let mut model = new_test_model();

    let event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Fetch,
        name: "large-file.tar.gz".to_string(),
        parent: None,
        detail: Some("https://example.com/large-file".to_string()),
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(event);

    let progress_event = ActivityEvent::Progress {
        id: 1,
        progress: ProgressState::Indeterminate {
            current: 42_000_000,
            unit: Some(ProgressUnit::Bytes),
        },
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(progress_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test deep nesting (3+ levels) shows hierarchy correctly.
#[test]
fn test_deep_nesting() {
    let mut model = new_test_model();

    // Level 0: Root evaluation
    let eval_event = ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Evaluate,
        name: "devenv.nix".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(eval_event);

    // Level 1: Build triggered during evaluation
    let build_event = ActivityEvent::Start {
        id: 2,
        kind: ActivityKind::Build,
        name: "wrapper-scripts".to_string(),
        parent: Some(1),
        detail: None,
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(build_event);
    model.apply_activity_event(ActivityEvent::Phase {
        id: 2,
        phase: "buildPhase".to_string(),
        timestamp: Timestamp::now(),
    });

    // Level 2: Fetch triggered during build
    let fetch_event = ActivityEvent::Start {
        id: 3,
        kind: ActivityKind::Fetch,
        name: "bash-5.2".to_string(),
        parent: Some(2),
        detail: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(fetch_event);
    model.apply_activity_event(ActivityEvent::Progress {
        id: 3,
        progress: ProgressState::Determinate {
            current: 500_000,
            total: 1_000_000,
            unit: Some(ProgressUnit::Bytes),
        },
        timestamp: Timestamp::now(),
    });

    // Level 3: Nested dependency fetch
    let nested_fetch = ActivityEvent::Start {
        id: 4,
        kind: ActivityKind::Fetch,
        name: "readline-8.2".to_string(),
        parent: Some(3),
        detail: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(nested_fetch);

    // Level 4: Even deeper
    let deep_fetch = ActivityEvent::Start {
        id: 5,
        kind: ActivityKind::Fetch,
        name: "ncurses-6.4".to_string(),
        parent: Some(4),
        detail: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    };
    model.apply_activity_event(deep_fetch);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test many concurrent activities (stress test for rendering).
#[test]
fn test_many_concurrent_activities() {
    let mut model = new_test_model();

    // Create 8 concurrent activities of various types
    for i in 0..8 {
        let (kind, name, detail, phase) = match i % 4 {
            0 => (
                ActivityKind::Build,
                format!("package-{}", i),
                None,
                Some("buildPhase"),
            ),
            1 => (
                ActivityKind::Fetch,
                format!("dependency-{}", i),
                Some("https://cache.nixos.org".to_string()),
                None,
            ),
            2 => (ActivityKind::Task, format!("task-{}", i), None, None),
            _ => (
                ActivityKind::Evaluate,
                format!("module-{}", i),
                None,
                None,
            ),
        };

        let event = ActivityEvent::Start {
            id: i as u64 + 1,
            kind,
            name,
            parent: None,
            detail,
            timestamp: Timestamp::now(),
        };
        model.apply_activity_event(event);

        if let Some(p) = phase {
            model.apply_activity_event(ActivityEvent::Phase {
                id: i as u64 + 1,
                phase: p.to_string(),
                timestamp: Timestamp::now(),
            });
        }

        // Add progress to fetch activities
        if i % 4 == 1 {
            model.apply_activity_event(ActivityEvent::Progress {
                id: i as u64 + 1,
                progress: ProgressState::Determinate {
                    current: (i as u64 + 1) * 1_000_000,
                    total: 10_000_000,
                    unit: Some(ProgressUnit::Bytes),
                },
                timestamp: Timestamp::now(),
            });
        }
    }

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test mixed completed and active activities.
#[test]
fn test_mixed_completed_and_active() {
    let mut model = new_test_model();

    // Completed build
    model.apply_activity_event(ActivityEvent::Start {
        id: 1,
        kind: ActivityKind::Build,
        name: "dependency-a".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(ActivityEvent::Complete {
        id: 1,
        outcome: devenv_activity::ActivityOutcome::Success,
        timestamp: Timestamp::now(),
    });

    // Failed build
    model.apply_activity_event(ActivityEvent::Start {
        id: 2,
        kind: ActivityKind::Build,
        name: "dependency-b".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(ActivityEvent::Complete {
        id: 2,
        outcome: devenv_activity::ActivityOutcome::Failed,
        timestamp: Timestamp::now(),
    });

    // Active build
    model.apply_activity_event(ActivityEvent::Start {
        id: 3,
        kind: ActivityKind::Build,
        name: "main-package".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(ActivityEvent::Phase {
        id: 3,
        phase: "buildPhase".to_string(),
        timestamp: Timestamp::now(),
    });

    // Active download
    model.apply_activity_event(ActivityEvent::Start {
        id: 4,
        kind: ActivityKind::Fetch,
        name: "runtime-dep".to_string(),
        parent: None,
        detail: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(ActivityEvent::Progress {
        id: 4,
        progress: ProgressState::Determinate {
            current: 3_000_000,
            total: 5_000_000,
            unit: Some(ProgressUnit::Bytes),
        },
        timestamp: Timestamp::now(),
    });

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

