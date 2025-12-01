//! TUI snapshot tests.
//!
//! These tests verify that when activity events are fed into the model,
//! the TUI renders the expected output.

use devenv_activity::{
    ActivityEvent, ActivityOutcome, Build, Evaluate, Fetch, FetchKind, Operation, Task, Timestamp,
};
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

    let event = ActivityEvent::Build(Build::Start {
        id: 1,
        name: "Building hello-world".to_string(),
        parent: None,
        derivation_path: None,
        timestamp: Timestamp::now(),
    });

    model.apply_activity_event(event);

    let phase_event = ActivityEvent::Build(Build::Phase {
        id: 1,
        phase: "buildPhase".to_string(),
        timestamp: Timestamp::now(),
    });

    model.apply_activity_event(phase_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test that a download activity with progress shows in the TUI.
#[test]
fn test_download_with_progress() {
    let mut model = new_test_model();

    let event = ActivityEvent::Fetch(Fetch::Start {
        id: 1,
        kind: FetchKind::Download,
        name: "Downloading nixpkgs".to_string(),
        parent: None,
        url: None,
        timestamp: Timestamp::now(),
    });

    model.apply_activity_event(event);

    let progress_event = ActivityEvent::Fetch(Fetch::Progress {
        id: 1,
        current: 5000,
        total: Some(10000),
        timestamp: Timestamp::now(),
    });

    model.apply_activity_event(progress_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test that a task activity shows in the TUI.
#[test]
fn test_task_running() {
    let mut model = new_test_model();

    let event = ActivityEvent::Task(Task::Start {
        id: 1,
        name: "Running tests".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    });

    model.apply_activity_event(event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test that multiple concurrent activities show in the TUI.
#[test]
fn test_multiple_activities() {
    let mut model = new_test_model();

    let build_event = ActivityEvent::Build(Build::Start {
        id: 1,
        name: "Building package-a".to_string(),
        parent: None,
        derivation_path: None,
        timestamp: Timestamp::now(),
    });

    let build_phase_event = ActivityEvent::Build(Build::Phase {
        id: 1,
        phase: "buildPhase".to_string(),
        timestamp: Timestamp::now(),
    });

    let download_event = ActivityEvent::Fetch(Fetch::Start {
        id: 2,
        kind: FetchKind::Download,
        name: "Downloading package-b".to_string(),
        parent: None,
        url: None,
        timestamp: Timestamp::now(),
    });

    let download_progress_event = ActivityEvent::Fetch(Fetch::Progress {
        id: 2,
        current: 2500,
        total: Some(5000),
        timestamp: Timestamp::now(),
    });

    let task_event = ActivityEvent::Task(Task::Start {
        id: 3,
        name: "Running setup".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    });

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

    let start_event = ActivityEvent::Task(Task::Start {
        id: 1,
        name: "Build completed".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    });

    model.apply_activity_event(start_event);

    let complete_event = ActivityEvent::Task(Task::Complete {
        id: 1,
        outcome: ActivityOutcome::Success,
        timestamp: Timestamp::now(),
    });

    model.apply_activity_event(complete_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test task failure shows in the TUI.
#[test]
fn test_task_failed() {
    let mut model = new_test_model();

    let start_event = ActivityEvent::Task(Task::Start {
        id: 1,
        name: "Tests failed".to_string(),
        parent: None,
        detail: None,
        timestamp: Timestamp::now(),
    });

    model.apply_activity_event(start_event);

    let complete_event = ActivityEvent::Task(Task::Complete {
        id: 1,
        outcome: ActivityOutcome::Failed,
        timestamp: Timestamp::now(),
    });

    model.apply_activity_event(complete_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test evaluating activity shows in the TUI.
#[test]
fn test_evaluating_activity() {
    let mut model = new_test_model();

    let event = ActivityEvent::Evaluate(Evaluate::Start {
        id: 1,
        name: "Evaluating flake".to_string(),
        parent: None,
        timestamp: Timestamp::now(),
    });

    model.apply_activity_event(event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test query activity shows in the TUI.
#[test]
fn test_query_activity() {
    let mut model = new_test_model();

    let event = ActivityEvent::Operation(Operation::Start {
        id: 1,
        name: "Querying cache".to_string(),
        parent: None,
        detail: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    });

    model.apply_activity_event(event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test fetch tree activity shows in the TUI.
#[test]
fn test_fetch_tree_activity() {
    let mut model = new_test_model();

    let event = ActivityEvent::Fetch(Fetch::Start {
        id: 1,
        kind: FetchKind::Tree,
        name: "Fetching github:NixOS/nixpkgs".to_string(),
        parent: None,
        url: None,
        timestamp: Timestamp::now(),
    });

    model.apply_activity_event(event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test download with substituter info shows in the TUI.
#[test]
fn test_download_with_substituter() {
    let mut model = new_test_model();

    let event = ActivityEvent::Fetch(Fetch::Start {
        id: 1,
        kind: FetchKind::Download,
        name: "Downloading package".to_string(),
        parent: None,
        url: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    });

    model.apply_activity_event(event);

    let progress_event = ActivityEvent::Fetch(Fetch::Progress {
        id: 1,
        current: 1000,
        total: Some(2000),
        timestamp: Timestamp::now(),
    });

    model.apply_activity_event(progress_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test that Nix evaluation with nested child activities (builds, fetches, downloads) shows hierarchy.
#[test]
fn test_nested_evaluation_with_children() {
    let mut model = new_test_model();

    // Parent: Nix evaluation
    let eval_event = ActivityEvent::Evaluate(Evaluate::Start {
        id: 100,
        name: "devenv.nix".to_string(),
        parent: None,
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(eval_event);

    // Child: Fetch triggered during evaluation
    let fetch_event = ActivityEvent::Fetch(Fetch::Start {
        id: 101,
        kind: FetchKind::Tree,
        name: "github:NixOS/nixpkgs".to_string(),
        parent: Some(100),
        url: None,
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(fetch_event);

    // Child: Build triggered during evaluation
    let build_event = ActivityEvent::Build(Build::Start {
        id: 102,
        name: "hello-2.12".to_string(),
        parent: Some(100),
        derivation_path: None,
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(build_event);

    let build_phase = ActivityEvent::Build(Build::Phase {
        id: 102,
        phase: "buildPhase".to_string(),
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(build_phase);

    // Child: Download triggered during evaluation
    let download_event = ActivityEvent::Fetch(Fetch::Start {
        id: 103,
        kind: FetchKind::Download,
        name: "openssl-3.0.0".to_string(),
        parent: Some(100),
        url: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(download_event);

    // Grandchild: Download triggered during build (nested 2 levels)
    let nested_download_event = ActivityEvent::Fetch(Fetch::Start {
        id: 104,
        kind: FetchKind::Download,
        name: "glibc-2.35".to_string(),
        parent: Some(102),
        url: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(nested_download_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test that activity details are stored correctly.
#[test]
fn test_activity_with_details() {
    let mut model = new_test_model();

    // Create an operation with a detail
    let parent_event = ActivityEvent::Operation(Operation::Start {
        id: 1,
        name: "Building shell".to_string(),
        parent: None,
        detail: Some("nix eval --json .#devenv.config".to_string()),
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(parent_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test multiple parallel builds running concurrently.
#[test]
fn test_multiple_parallel_builds() {
    let mut model = new_test_model();

    // Start multiple builds at different phases
    let build1 = ActivityEvent::Build(Build::Start {
        id: 1,
        name: "hello-2.12".to_string(),
        parent: None,
        derivation_path: None,
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(build1);
    model.apply_activity_event(ActivityEvent::Build(Build::Phase {
        id: 1,
        phase: "buildPhase".to_string(),
        timestamp: Timestamp::now(),
    }));

    let build2 = ActivityEvent::Build(Build::Start {
        id: 2,
        name: "openssl-3.0.0".to_string(),
        parent: None,
        derivation_path: None,
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(build2);
    model.apply_activity_event(ActivityEvent::Build(Build::Phase {
        id: 2,
        phase: "configurePhase".to_string(),
        timestamp: Timestamp::now(),
    }));

    let build3 = ActivityEvent::Build(Build::Start {
        id: 3,
        name: "python-3.11.5".to_string(),
        parent: None,
        derivation_path: None,
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(build3);
    model.apply_activity_event(ActivityEvent::Build(Build::Phase {
        id: 3,
        phase: "installPhase".to_string(),
        timestamp: Timestamp::now(),
    }));

    let build4 = ActivityEvent::Build(Build::Start {
        id: 4,
        name: "gcc-12.3.0".to_string(),
        parent: None,
        derivation_path: None,
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(build4);
    model.apply_activity_event(ActivityEvent::Build(Build::Phase {
        id: 4,
        phase: "unpackPhase".to_string(),
        timestamp: Timestamp::now(),
    }));

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test parallel downloads and builds happening simultaneously.
#[test]
fn test_parallel_downloads_and_builds() {
    let mut model = new_test_model();

    // Two builds running
    let build1 = ActivityEvent::Build(Build::Start {
        id: 1,
        name: "hello-2.12".to_string(),
        parent: None,
        derivation_path: None,
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(build1);
    model.apply_activity_event(ActivityEvent::Build(Build::Phase {
        id: 1,
        phase: "buildPhase".to_string(),
        timestamp: Timestamp::now(),
    }));

    let build2 = ActivityEvent::Build(Build::Start {
        id: 2,
        name: "curl-8.1.0".to_string(),
        parent: None,
        derivation_path: None,
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(build2);
    model.apply_activity_event(ActivityEvent::Build(Build::Phase {
        id: 2,
        phase: "configurePhase".to_string(),
        timestamp: Timestamp::now(),
    }));

    // Three downloads in progress
    let download1 = ActivityEvent::Fetch(Fetch::Start {
        id: 3,
        kind: FetchKind::Download,
        name: "openssl-3.0.0".to_string(),
        parent: None,
        url: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(download1);
    model.apply_activity_event(ActivityEvent::Fetch(Fetch::Progress {
        id: 3,
        current: 15_000_000,
        total: Some(30_000_000),
        timestamp: Timestamp::now(),
    }));

    let download2 = ActivityEvent::Fetch(Fetch::Start {
        id: 4,
        kind: FetchKind::Download,
        name: "glibc-2.37".to_string(),
        parent: None,
        url: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(download2);
    model.apply_activity_event(ActivityEvent::Fetch(Fetch::Progress {
        id: 4,
        current: 8_000_000,
        total: Some(10_000_000),
        timestamp: Timestamp::now(),
    }));

    let download3 = ActivityEvent::Fetch(Fetch::Start {
        id: 5,
        kind: FetchKind::Download,
        name: "python-3.11.5".to_string(),
        parent: None,
        url: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(download3);
    model.apply_activity_event(ActivityEvent::Fetch(Fetch::Progress {
        id: 5,
        current: 1_000_000,
        total: Some(50_000_000),
        timestamp: Timestamp::now(),
    }));

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test indeterminate progress shows in the TUI.
#[test]
fn test_indeterminate_progress() {
    let mut model = new_test_model();

    let event = ActivityEvent::Fetch(Fetch::Start {
        id: 1,
        kind: FetchKind::Download,
        name: "large-file.tar.gz".to_string(),
        parent: None,
        url: Some("https://example.com/large-file".to_string()),
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(event);

    let progress_event = ActivityEvent::Fetch(Fetch::Progress {
        id: 1,
        current: 42_000_000,
        total: None,
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(progress_event);

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test deep nesting (3+ levels) shows hierarchy correctly.
#[test]
fn test_deep_nesting() {
    let mut model = new_test_model();

    // Level 0: Root evaluation
    let eval_event = ActivityEvent::Evaluate(Evaluate::Start {
        id: 1,
        name: "devenv.nix".to_string(),
        parent: None,
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(eval_event);

    // Level 1: Build triggered during evaluation
    let build_event = ActivityEvent::Build(Build::Start {
        id: 2,
        name: "wrapper-scripts".to_string(),
        parent: Some(1),
        derivation_path: None,
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(build_event);
    model.apply_activity_event(ActivityEvent::Build(Build::Phase {
        id: 2,
        phase: "buildPhase".to_string(),
        timestamp: Timestamp::now(),
    }));

    // Level 2: Fetch triggered during build
    let fetch_event = ActivityEvent::Fetch(Fetch::Start {
        id: 3,
        kind: FetchKind::Download,
        name: "bash-5.2".to_string(),
        parent: Some(2),
        url: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(fetch_event);
    model.apply_activity_event(ActivityEvent::Fetch(Fetch::Progress {
        id: 3,
        current: 500_000,
        total: Some(1_000_000),
        timestamp: Timestamp::now(),
    }));

    // Level 3: Nested dependency fetch
    let nested_fetch = ActivityEvent::Fetch(Fetch::Start {
        id: 4,
        kind: FetchKind::Download,
        name: "readline-8.2".to_string(),
        parent: Some(3),
        url: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    });
    model.apply_activity_event(nested_fetch);

    // Level 4: Even deeper
    let deep_fetch = ActivityEvent::Fetch(Fetch::Start {
        id: 5,
        kind: FetchKind::Download,
        name: "ncurses-6.4".to_string(),
        parent: Some(4),
        url: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    });
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
        match i % 4 {
            0 => {
                model.apply_activity_event(ActivityEvent::Build(Build::Start {
                    id: i as u64 + 1,
                    name: format!("package-{}", i),
                    parent: None,
                    derivation_path: None,
                    timestamp: Timestamp::now(),
                }));
                model.apply_activity_event(ActivityEvent::Build(Build::Phase {
                    id: i as u64 + 1,
                    phase: "buildPhase".to_string(),
                    timestamp: Timestamp::now(),
                }));
            }
            1 => {
                model.apply_activity_event(ActivityEvent::Fetch(Fetch::Start {
                    id: i as u64 + 1,
                    kind: FetchKind::Download,
                    name: format!("dependency-{}", i),
                    parent: None,
                    url: Some("https://cache.nixos.org".to_string()),
                    timestamp: Timestamp::now(),
                }));
                model.apply_activity_event(ActivityEvent::Fetch(Fetch::Progress {
                    id: i as u64 + 1,
                    current: (i as u64 + 1) * 1_000_000,
                    total: Some(10_000_000),
                    timestamp: Timestamp::now(),
                }));
            }
            2 => {
                model.apply_activity_event(ActivityEvent::Task(Task::Start {
                    id: i as u64 + 1,
                    name: format!("task-{}", i),
                    parent: None,
                    detail: None,
                    timestamp: Timestamp::now(),
                }));
            }
            _ => {
                model.apply_activity_event(ActivityEvent::Evaluate(Evaluate::Start {
                    id: i as u64 + 1,
                    name: format!("module-{}", i),
                    parent: None,
                    timestamp: Timestamp::now(),
                }));
            }
        };
    }

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

/// Test mixed completed and active activities.
#[test]
fn test_mixed_completed_and_active() {
    let mut model = new_test_model();

    // Completed build
    model.apply_activity_event(ActivityEvent::Build(Build::Start {
        id: 1,
        name: "dependency-a".to_string(),
        parent: None,
        derivation_path: None,
        timestamp: Timestamp::now(),
    }));
    model.apply_activity_event(ActivityEvent::Build(Build::Complete {
        id: 1,
        outcome: ActivityOutcome::Success,
        timestamp: Timestamp::now(),
    }));

    // Failed build
    model.apply_activity_event(ActivityEvent::Build(Build::Start {
        id: 2,
        name: "dependency-b".to_string(),
        parent: None,
        derivation_path: None,
        timestamp: Timestamp::now(),
    }));
    model.apply_activity_event(ActivityEvent::Build(Build::Complete {
        id: 2,
        outcome: ActivityOutcome::Failed,
        timestamp: Timestamp::now(),
    }));

    // Active build
    model.apply_activity_event(ActivityEvent::Build(Build::Start {
        id: 3,
        name: "main-package".to_string(),
        parent: None,
        derivation_path: None,
        timestamp: Timestamp::now(),
    }));
    model.apply_activity_event(ActivityEvent::Build(Build::Phase {
        id: 3,
        phase: "buildPhase".to_string(),
        timestamp: Timestamp::now(),
    }));

    // Active download
    model.apply_activity_event(ActivityEvent::Fetch(Fetch::Start {
        id: 4,
        kind: FetchKind::Download,
        name: "runtime-dep".to_string(),
        parent: None,
        url: Some("https://cache.nixos.org".to_string()),
        timestamp: Timestamp::now(),
    }));
    model.apply_activity_event(ActivityEvent::Fetch(Fetch::Progress {
        id: 4,
        current: 3_000_000,
        total: Some(5_000_000),
        timestamp: Timestamp::now(),
    }));

    let output = render_to_string(&model);
    insta::assert_snapshot!(output);
}

