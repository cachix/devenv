#![cfg(feature = "test-all")]
//! TUI snapshot tests.
//!
//! These tests verify that when activity events are fed into the model,
//! the TUI renders the expected output.

use devenv_activity::test_helpers::*;
use devenv_activity::{ActivityLevel, ActivityOutcome, FetchKind, TaskInfo};
use devenv_tui::{ActivityModel, RenderContext, UiState, view::view};
use iocraft::prelude::*;

const TEST_WIDTH: u16 = 80;
const TEST_HEIGHT: u16 = 24;

fn render_to_string(model: &ActivityModel, ui_state: &UiState) -> String {
    let mut element = view(model, ui_state, RenderContext::Normal, None, false).into();
    element.render(Some(TEST_WIDTH as usize)).to_string()
}

fn new_test_model() -> (ActivityModel, UiState) {
    let model = ActivityModel::new();
    let mut ui_state = UiState::new();
    ui_state.set_terminal_size(TEST_WIDTH, TEST_HEIGHT);
    (model, ui_state)
}

/// Test that an empty model renders correctly.
#[test]
fn test_empty_model() {
    let (model, ui_state) = new_test_model();
    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that a single build activity shows in the TUI.
#[test]
fn test_single_build_activity() {
    let (mut model, ui_state) = new_test_model();

    let event = build_start(1, "Building hello-world");

    model.apply_activity_event(event);

    let phase_event = build_phase(1, "buildPhase");

    model.apply_activity_event(phase_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that a download activity with progress shows in the TUI.
#[test]
fn test_download_with_progress() {
    let (mut model, ui_state) = new_test_model();

    let event = fetch_start(1, FetchKind::Download, "Downloading nixpkgs");

    model.apply_activity_event(event);

    let progress_event = fetch_progress(1, 5000, Some(10000));

    model.apply_activity_event(progress_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that a task activity shows in the TUI.
#[test]
fn test_task_running() {
    let (mut model, ui_state) = new_test_model();

    // First emit hierarchy, then start
    model.apply_activity_event(task_hierarchy_single(
        1,
        "Running tests",
        None,
        false,
        false,
    ));
    model.apply_activity_event(task_start(1));

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that multiple concurrent activities show in the TUI.
#[test]
fn test_multiple_activities() {
    let (mut model, ui_state) = new_test_model();

    let build_event = build_start(1, "Building package-a");

    let build_phase_event = build_phase(1, "buildPhase");

    let download_event = fetch_start(2, FetchKind::Download, "Downloading package-b");

    let download_progress_event = fetch_progress(2, 2500, Some(5000));

    model.apply_activity_event(build_event);
    model.apply_activity_event(build_phase_event);
    model.apply_activity_event(download_event);
    model.apply_activity_event(download_progress_event);
    model.apply_activity_event(task_hierarchy_single(
        3,
        "Running setup",
        None,
        false,
        false,
    ));
    model.apply_activity_event(task_start(3));

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test task status lifecycle: pending -> running -> success.
#[test]
fn test_task_success() {
    let (mut model, ui_state) = new_test_model();

    model.apply_activity_event(task_hierarchy_single(
        1,
        "Build completed",
        None,
        false,
        false,
    ));
    model.apply_activity_event(task_start(1));

    let complete_event = task_complete(1, ActivityOutcome::Success);

    model.apply_activity_event(complete_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test task failure shows in the TUI.
#[test]
fn test_task_failed() {
    let (mut model, ui_state) = new_test_model();

    model.apply_activity_event(task_hierarchy_single(1, "Tests failed", None, false, false));
    model.apply_activity_event(task_start(1));

    let complete_event = task_complete(1, ActivityOutcome::Failed);

    model.apply_activity_event(complete_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that failed tasks show logs even when show_output=false.
#[test]
fn test_task_failed_shows_logs() {
    let (mut model, ui_state) = new_test_model();

    model.apply_activity_event(task_hierarchy_single(
        1,
        "test:failing-task",
        None,
        false,
        false,
    ));
    model.apply_activity_event(task_start(1));

    // Send log events - these should be visible because the task fails
    model.apply_activity_event(task_log(1, "Running test suite...", false));
    model.apply_activity_event(task_log(1, "FAILED: assertion error in test_foo", true));
    model.apply_activity_event(task_log(1, "Expected: 42, Got: 0", true));

    let complete_event = task_complete(1, ActivityOutcome::Failed);
    model.apply_activity_event(complete_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that task with show_output=true displays logs in the TUI.
#[test]
fn test_task_show_output_true() {
    let (mut model, ui_state) = new_test_model();

    model.apply_activity_event(task_hierarchy_single(
        1,
        "test:with-output",
        None,
        true,
        false,
    ));
    model.apply_activity_event(task_start(1));

    // Send log events
    model.apply_activity_event(task_log(1, "VISIBLE_OUTPUT_LINE_1", false));
    model.apply_activity_event(task_log(1, "VISIBLE_OUTPUT_LINE_2", false));

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that task with show_output=false hides logs in the TUI.
#[test]
fn test_task_show_output_false() {
    let (mut model, ui_state) = new_test_model();

    model.apply_activity_event(task_hierarchy_single(
        1,
        "test:without-output",
        None,
        false,
        false,
    ));
    model.apply_activity_event(task_start(1));

    // Send log events - these should be filtered out
    model.apply_activity_event(task_log(1, "HIDDEN_OUTPUT_LINE_1", false));
    model.apply_activity_event(task_log(1, "HIDDEN_OUTPUT_LINE_2", false));

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test evaluating activity shows in the TUI.
#[test]
fn test_evaluating_activity() {
    let (mut model, ui_state) = new_test_model();

    let event = evaluate_start(1, "Building shell", ActivityLevel::Info);

    model.apply_activity_event(event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test query activity shows in the TUI.
#[test]
fn test_query_activity() {
    let (mut model, ui_state) = new_test_model();

    let event = fetch_start_with(
        1,
        FetchKind::Query,
        "7xyndmr0mgfissin0h5ggzb0b2i5drbz-cargo-vendor-dir",
        None,
        Some("https://some-cache.org"),
    );

    model.apply_activity_event(event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test fetch tree activity shows in the TUI.
#[test]
fn test_fetch_tree_activity() {
    let (mut model, ui_state) = new_test_model();

    let event = fetch_start(1, FetchKind::Tree, "Fetching github:NixOS/nixpkgs");

    model.apply_activity_event(event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test download with substituter info shows in the TUI.
#[test]
fn test_download_with_substituter() {
    let (mut model, ui_state) = new_test_model();

    let event = fetch_start_with(
        1,
        FetchKind::Download,
        "Downloading package",
        None,
        Some("https://cache.nixos.org"),
    );

    model.apply_activity_event(event);

    let progress_event = fetch_progress(1, 1000, Some(2000));

    model.apply_activity_event(progress_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that Nix evaluation with nested child activities (builds, fetches, downloads) shows hierarchy.
#[test]
fn test_nested_evaluation_with_children() {
    let (mut model, ui_state) = new_test_model();

    // Parent: Nix evaluation
    let eval_event = evaluate_start(100, "Building shell", ActivityLevel::Info);
    model.apply_activity_event(eval_event);

    // Child: Fetch triggered during evaluation
    let fetch_event = fetch_start_with(
        101,
        FetchKind::Tree,
        "github:NixOS/nixpkgs",
        Some(100),
        None,
    );
    model.apply_activity_event(fetch_event);

    // Child: Build triggered during evaluation
    let build_event = build_start_with(102, "hello-2.12", Some(100));
    model.apply_activity_event(build_event);

    let build_phase = build_phase(102, "buildPhase");
    model.apply_activity_event(build_phase);

    // Child: Download triggered during evaluation
    let download_event = fetch_start_with(
        103,
        FetchKind::Download,
        "openssl-3.0.0",
        Some(100),
        Some("https://cache.nixos.org"),
    );
    model.apply_activity_event(download_event);

    // Grandchild: Download triggered during build (nested 2 levels)
    let nested_download_event = fetch_start_with(
        104,
        FetchKind::Download,
        "glibc-2.35",
        Some(102),
        Some("https://cache.nixos.org"),
    );
    model.apply_activity_event(nested_download_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that activity details are stored correctly.
#[test]
fn test_activity_with_details() {
    let (mut model, ui_state) = new_test_model();

    // Create an operation with a detail
    let parent_event = operation_start_with(
        1,
        "Building shell",
        None,
        Some("nix eval --json .#devenv.config"),
        ActivityLevel::Info,
    );
    model.apply_activity_event(parent_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test multiple parallel builds running concurrently.
#[test]
fn test_multiple_parallel_builds() {
    let (mut model, ui_state) = new_test_model();

    // Start multiple builds at different phases
    let build1 = build_start(1, "hello-2.12");
    model.apply_activity_event(build1);
    model.apply_activity_event(build_phase(1, "buildPhase"));

    let build2 = build_start(2, "openssl-3.0.0");
    model.apply_activity_event(build2);
    model.apply_activity_event(build_phase(2, "configurePhase"));

    let build3 = build_start(3, "python-3.11.5");
    model.apply_activity_event(build3);
    model.apply_activity_event(build_phase(3, "installPhase"));

    let build4 = build_start(4, "gcc-12.3.0");
    model.apply_activity_event(build4);
    model.apply_activity_event(build_phase(4, "unpackPhase"));

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test parallel downloads and builds happening simultaneously.
#[test]
fn test_parallel_downloads_and_builds() {
    let (mut model, ui_state) = new_test_model();

    // Two builds running
    let build1 = build_start(1, "hello-2.12");
    model.apply_activity_event(build1);
    model.apply_activity_event(build_phase(1, "buildPhase"));

    let build2 = build_start(2, "curl-8.1.0");
    model.apply_activity_event(build2);
    model.apply_activity_event(build_phase(2, "configurePhase"));

    // Three downloads in progress
    let download1 = fetch_start_with(
        3,
        FetchKind::Download,
        "openssl-3.0.0",
        None,
        Some("https://cache.nixos.org"),
    );
    model.apply_activity_event(download1);
    model.apply_activity_event(fetch_progress(3, 15_000_000, Some(30_000_000)));

    let download2 = fetch_start_with(
        4,
        FetchKind::Download,
        "glibc-2.37",
        None,
        Some("https://cache.nixos.org"),
    );
    model.apply_activity_event(download2);
    model.apply_activity_event(fetch_progress(4, 8_000_000, Some(10_000_000)));

    let download3 = fetch_start_with(
        5,
        FetchKind::Download,
        "python-3.11.5",
        None,
        Some("https://cache.nixos.org"),
    );
    model.apply_activity_event(download3);
    model.apply_activity_event(fetch_progress(5, 1_000_000, Some(50_000_000)));

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test indeterminate progress shows in the TUI.
#[test]
fn test_indeterminate_progress() {
    let (mut model, ui_state) = new_test_model();

    let event = fetch_start_with(
        1,
        FetchKind::Download,
        "large-file.tar.gz",
        None,
        Some("https://example.com/large-file"),
    );
    model.apply_activity_event(event);

    let progress_event = fetch_progress(1, 42_000_000, None);
    model.apply_activity_event(progress_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test deep nesting (3+ levels) shows hierarchy correctly.
#[test]
fn test_deep_nesting() {
    let (mut model, ui_state) = new_test_model();

    // Level 0: Root evaluation
    let eval_event = evaluate_start(1, "Building shell", ActivityLevel::Info);
    model.apply_activity_event(eval_event);

    // Level 1: Build triggered during evaluation
    let build_event = build_start_with(2, "wrapper-scripts", Some(1));
    model.apply_activity_event(build_event);
    model.apply_activity_event(build_phase(2, "buildPhase"));

    // Level 2: Fetch triggered during build
    let fetch_event = fetch_start_with(
        3,
        FetchKind::Download,
        "bash-5.2",
        Some(2),
        Some("https://cache.nixos.org"),
    );
    model.apply_activity_event(fetch_event);
    model.apply_activity_event(fetch_progress(3, 500_000, Some(1_000_000)));

    // Level 3: Nested dependency fetch
    let nested_fetch = fetch_start_with(
        4,
        FetchKind::Download,
        "readline-8.2",
        Some(3),
        Some("https://cache.nixos.org"),
    );
    model.apply_activity_event(nested_fetch);

    // Level 4: Even deeper
    let deep_fetch = fetch_start_with(
        5,
        FetchKind::Download,
        "ncurses-6.4",
        Some(4),
        Some("https://cache.nixos.org"),
    );
    model.apply_activity_event(deep_fetch);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test many concurrent activities (stress test for rendering).
#[test]
fn test_many_concurrent_activities() {
    let (mut model, ui_state) = new_test_model();

    // Create 8 concurrent activities of various types
    for i in 0..8 {
        match i % 4 {
            0 => {
                model.apply_activity_event(build_start(i as u64 + 1, format!("package-{}", i)));
                model.apply_activity_event(build_phase(i as u64 + 1, "buildPhase"));
            }
            1 => {
                model.apply_activity_event(fetch_start_with(
                    i as u64 + 1,
                    FetchKind::Download,
                    format!("dependency-{}", i),
                    None,
                    Some("https://cache.nixos.org"),
                ));
                model.apply_activity_event(fetch_progress(
                    i as u64 + 1,
                    (i as u64 + 1) * 1_000_000,
                    Some(10_000_000),
                ));
            }
            2 => {
                model.apply_activity_event(task_hierarchy_single(
                    i as u64 + 1,
                    &format!("task-{}", i),
                    None,
                    false,
                    false,
                ));
                model.apply_activity_event(task_start(i as u64 + 1));
            }
            _ => {
                model.apply_activity_event(evaluate_start(
                    i as u64 + 1,
                    "Building shell",
                    ActivityLevel::Info,
                ));
            }
        };
    }

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test mixed completed and active activities.
#[test]
fn test_mixed_completed_and_active() {
    let (mut model, ui_state) = new_test_model();

    // Completed build
    model.apply_activity_event(build_start(1, "dependency-a"));
    model.apply_activity_event(build_complete(1, ActivityOutcome::Success));

    // Failed build
    model.apply_activity_event(build_start(2, "dependency-b"));
    model.apply_activity_event(build_complete(2, ActivityOutcome::Failed));

    // Active build
    model.apply_activity_event(build_start(3, "main-package"));
    model.apply_activity_event(build_phase(3, "buildPhase"));

    // Active download
    model.apply_activity_event(fetch_start_with(
        4,
        FetchKind::Download,
        "runtime-dep",
        None,
        Some("https://cache.nixos.org"),
    ));
    model.apply_activity_event(fetch_progress(4, 3_000_000, Some(5_000_000)));

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that standalone error messages (without parent) show in the TUI.
#[test]
fn test_standalone_error_message() {
    let (mut model, ui_state) = new_test_model();

    // Add a standalone error message (no parent activity)
    let error_event = message_with(
        100,
        ActivityLevel::Error,
        "error: attribute 'nonExistentPackage' not found",
        None,
    );
    model.apply_activity_event(error_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that multiple error messages show in the TUI.
#[test]
fn test_multiple_error_messages() {
    let (mut model, ui_state) = new_test_model();

    // Add multiple standalone error messages
    let error1 = message_with(
        100,
        ActivityLevel::Error,
        "error: attribute 'foo' not found",
        None,
    );
    model.apply_activity_event(error1);

    let error2 = message_with(
        101,
        ActivityLevel::Error,
        "error: while evaluating 'bar': infinite recursion",
        None,
    );
    model.apply_activity_event(error2);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that error messages with parent activities show as child activities.
#[test]
fn test_error_message_with_parent() {
    let (mut model, ui_state) = new_test_model();

    // Start an evaluation
    let eval_event = evaluate_start(1, "Building shell", ActivityLevel::Info);
    model.apply_activity_event(eval_event);

    // Add an error message attached to the evaluation
    let error_event = message_with(
        100,
        ActivityLevel::Error,
        "error: undefined variable 'pkgs'",
        Some(1),
    );
    model.apply_activity_event(error_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that warning messages with parent activities show as child activities.
#[test]
fn test_warning_message_with_parent() {
    let (mut model, ui_state) = new_test_model();

    // Start an evaluation
    let eval_event = evaluate_start(1, "Building shell", ActivityLevel::Info);
    model.apply_activity_event(eval_event);

    // Add a warning message attached to the evaluation
    let warn_event = message_with(
        100,
        ActivityLevel::Warn,
        "warning: deprecated option 'services.foo' used",
        Some(1),
    );
    model.apply_activity_event(warn_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test error messages alongside active builds.
#[test]
fn test_error_with_active_builds() {
    let (mut model, ui_state) = new_test_model();

    // Start a build
    let build_event = build_start(1, "hello-2.12");
    model.apply_activity_event(build_event);
    model.apply_activity_event(build_phase(1, "buildPhase"));

    // Add an error message
    let error_event = message_with(
        100,
        ActivityLevel::Error,
        "error: builder for '/nix/store/...-hello.drv' failed",
        None,
    );
    model.apply_activity_event(error_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that error messages with details show expansion indicator.
#[test]
fn test_error_message_with_details() {
    let (mut model, ui_state) = new_test_model();

    // Start an evaluation
    let eval_event = evaluate_start(1, "Building shell", ActivityLevel::Info);
    model.apply_activity_event(eval_event);

    // Add an error message with details (stack trace)
    let error_event = message_with_details(
        100,
        ActivityLevel::Error,
        "error: undefined variable 'pkgs'",
        Some(
            "error:\n       … while evaluating\n         at devenv.nix:10:5\n\n       error: undefined variable 'pkgs'",
        ),
        Some(1),
    );
    model.apply_activity_event(error_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that error messages without details don't show the [+] indicator.
#[test]
fn test_error_message_without_details() {
    let (mut model, ui_state) = new_test_model();

    // Start an evaluation
    let eval_event = evaluate_start(1, "Building shell", ActivityLevel::Info);
    model.apply_activity_event(eval_event);

    // Add an error message without details
    let error_event = message_with(100, ActivityLevel::Error, "error: simple error", Some(1));
    model.apply_activity_event(error_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that failed evaluations show error details inline (not hidden behind navigation).
/// The error details must be propagated from the child Message to the parent Evaluate
/// activity's build_logs, because the child expires after the linger duration and gets
/// pushed out by newer children (max_lines=5 default).
/// Regression test for https://github.com/cachix/devenv/issues/2720
#[test]
fn test_evaluate_failed_shows_error_details() {
    let (mut model, ui_state) = new_test_model();

    // Start an evaluation
    let eval_event = evaluate_start(1, "Evaluating shell", ActivityLevel::Info);
    model.apply_activity_event(eval_event);

    // Add an error message with details (as nix_log_bridge does for eval errors)
    let error_event = message_with_details(
        100,
        ActivityLevel::Error,
        "Evaluation error: Failed to get drvPath from shell derivation",
        Some(
            "… while evaluating the option `packages':\n\
             … while evaluating definitions from `languages/rust.nix':\n\
             \n\
             error: To use 'languages.rust.channel', run the following command:\n\
             \n\
             $ devenv inputs add rust-overlay github:oxalica/rust-overlay --follows nixpkgs",
        ),
        Some(1),
    );
    model.apply_activity_event(error_event);

    // Complete the evaluation with failure
    let complete_event = evaluate_complete(1, ActivityOutcome::Failed);
    model.apply_activity_event(complete_event);

    // Simulate the child error message expiring: backdate it past the linger duration,
    // then add 5 lingering siblings to fill max_lines and push the expired error out.
    if let Some(child) = model.activities.get_mut(&100) {
        child.completed_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(10));
    }
    for i in 0..5 {
        model.apply_activity_event(message_with(
            200 + i,
            ActivityLevel::Info,
            format!("evaluating file {i}"),
            Some(1),
        ));
    }

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that failed devenv operations show logs automatically (not just when selected).
#[test]
fn test_devenv_failed_shows_logs() {
    let (mut model, ui_state) = new_test_model();

    // Start a devenv operation
    let start_event = operation_start_with(1, "devenv shell", None, None, ActivityLevel::Info);
    model.apply_activity_event(start_event);

    // Send log events - these should be visible because the operation fails
    model.apply_activity_event(operation_log(1, "Running enterShell hook...", false));
    model.apply_activity_event(operation_log(1, "error: command 'foo' not found", true));
    model.apply_activity_event(operation_log(1, "hint: did you mean 'bar'?", true));

    // Complete with failure
    let complete_event = operation_complete(1, ActivityOutcome::Failed);
    model.apply_activity_event(complete_event);

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that a task with additional_parents appears under multiple parents.
#[test]
fn test_task_additional_parents() {
    let (mut model, ui_state) = new_test_model();

    // Emit hierarchy: two parent tasks and a shared dependency
    // The "build" task (id=3) has primary parent 1 (test:unit) and additional parent 2 (test:integration)
    let hierarchy = task_hierarchy(
        vec![
            TaskInfo {
                id: 1,
                name: "test:unit".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 2,
                name: "test:integration".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 3,
                name: "build".to_string(),
                show_output: false,
                is_process: false,
            },
        ],
        vec![
            // build appears under both test:unit (primary) and test:integration (additional)
            (1, 3), // test:unit -> build
            (2, 3), // test:integration -> build
        ],
    );
    model.apply_activity_event(hierarchy);

    // Start all tasks
    model.apply_activity_event(task_start(1));
    model.apply_activity_event(task_start(2));
    model.apply_activity_event(task_start(3));

    let output = render_to_string(&model, &ui_state);
    // The "build" task should appear under both test:unit and test:integration
    insta::assert_snapshot!(output);
}

#[test]
fn test_selectable_ids_dedup_for_multi_parent_tasks() {
    let (mut model, mut ui_state) = new_test_model();

    let hierarchy = task_hierarchy(
        vec![
            TaskInfo {
                id: 1,
                name: "parent:a".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 2,
                name: "parent:b".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 3,
                name: "shared".to_string(),
                show_output: false,
                is_process: false,
            },
        ],
        vec![
            (1, 3), // parent:a -> shared (primary)
            (2, 3), // parent:b -> shared (additional)
        ],
    );
    model.apply_activity_event(hierarchy);

    // Add logs so tasks 1 and 3 are selectable.
    model.apply_activity_event(task_log(1, "log-1", false));
    model.apply_activity_event(task_log(3, "log-3", false));

    let selectable = model.get_selectable_activity_ids(&ui_state);
    assert_eq!(selectable, vec![1, 3]);

    ui_state.select_activity(&selectable, true);
    assert_eq!(ui_state.selected_activity, Some(1));

    ui_state.select_activity(&selectable, true);
    assert_eq!(ui_state.selected_activity, Some(3));

    ui_state.select_activity(&selectable, true);
    assert_eq!(ui_state.selected_activity, Some(3));

    ui_state.select_activity(&selectable, false);
    assert_eq!(ui_state.selected_activity, Some(1));
}

/// Test that tasks in Queued state (hierarchy emitted but not started) render correctly.
#[test]
fn test_task_queued_state() {
    let (mut model, ui_state) = new_test_model();

    // Emit hierarchy for three tasks but only start one
    let hierarchy = task_hierarchy(
        vec![
            TaskInfo {
                id: 1,
                name: "test:first".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 2,
                name: "test:second".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 3,
                name: "test:third".to_string(),
                show_output: false,
                is_process: false,
            },
        ],
        vec![],
    );
    model.apply_activity_event(hierarchy);

    // Only start the first task - others remain in Queued state
    model.apply_activity_event(task_start(1));

    let output = render_to_string(&model, &ui_state);
    // First task should show as running, others should show as pending/queued
    insta::assert_snapshot!(output);
}

/// Test that a task that completes without ever starting (skipped) shows correctly.
#[test]
fn test_task_skipped_never_started() {
    let (mut model, ui_state) = new_test_model();

    // Emit hierarchy
    model.apply_activity_event(task_hierarchy_single(
        1,
        "test:skipped-task",
        None,
        false,
        false,
    ));

    // Complete with Skipped without ever calling task_start
    // This simulates a task that was skipped due to caching or no command
    model.apply_activity_event(task_complete(1, ActivityOutcome::Skipped));

    let output = render_to_string(&model, &ui_state);
    // Task should show as skipped with zero duration
    insta::assert_snapshot!(output);
}

/// Test that a task cancelled due to dependency failure shows correctly.
#[test]
fn test_task_dependency_failed_never_started() {
    let (mut model, ui_state) = new_test_model();

    // Emit hierarchy with dependency relationship
    let hierarchy = task_hierarchy(
        vec![
            TaskInfo {
                id: 1,
                name: "test:dep".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 2,
                name: "test:dependent".to_string(),
                show_output: false,
                is_process: false,
            },
        ],
        vec![(2, 1)], // test:dependent depends on test:dep
    );
    model.apply_activity_event(hierarchy);

    // Start and fail the dependency
    model.apply_activity_event(task_start(1));
    model.apply_activity_event(task_complete(1, ActivityOutcome::Failed));

    // The dependent task never starts, just gets DependencyFailed
    model.apply_activity_event(task_complete(2, ActivityOutcome::DependencyFailed));

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test task hierarchy with 3+ levels of nesting.
#[test]
fn test_task_deep_nesting() {
    let (mut model, ui_state) = new_test_model();

    // Create a deep hierarchy: root -> level1 -> level2 -> level3
    let hierarchy = task_hierarchy(
        vec![
            TaskInfo {
                id: 1,
                name: "test:root".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 2,
                name: "test:level1".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 3,
                name: "test:level2".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 4,
                name: "test:level3".to_string(),
                show_output: false,
                is_process: false,
            },
        ],
        vec![
            (1, 2), // root -> level1
            (2, 3), // level1 -> level2
            (3, 4), // level2 -> level3
        ],
    );
    model.apply_activity_event(hierarchy);

    // Start all tasks
    model.apply_activity_event(task_start(1));
    model.apply_activity_event(task_start(2));
    model.apply_activity_event(task_start(3));
    model.apply_activity_event(task_start(4));

    let output = render_to_string(&model, &ui_state);
    // Should show proper indentation for each nesting level
    insta::assert_snapshot!(output);
}

/// Test multiple tasks under the same parent render in consistent order.
#[test]
fn test_task_multiple_under_same_parent() {
    let (mut model, ui_state) = new_test_model();

    // Create a parent with multiple children
    let hierarchy = task_hierarchy(
        vec![
            TaskInfo {
                id: 1,
                name: "test:parent".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 2,
                name: "test:child-a".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 3,
                name: "test:child-b".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 4,
                name: "test:child-c".to_string(),
                show_output: false,
                is_process: false,
            },
        ],
        vec![
            (1, 2), // parent -> child-a
            (1, 3), // parent -> child-b
            (1, 4), // parent -> child-c
        ],
    );
    model.apply_activity_event(hierarchy);

    // Start parent and all children
    model.apply_activity_event(task_start(1));
    model.apply_activity_event(task_start(2));
    model.apply_activity_event(task_start(3));
    model.apply_activity_event(task_start(4));

    let output = render_to_string(&model, &ui_state);
    // Children should appear under parent in consistent order
    insta::assert_snapshot!(output);
}

/// Test diamond dependency pattern where a shared dependency appears under multiple parents.
#[test]
fn test_task_diamond_dependency() {
    let (mut model, ui_state) = new_test_model();

    // Diamond pattern:
    //     root
    //    /    \
    //   A      B
    //    \    /
    //     shared
    let hierarchy = task_hierarchy(
        vec![
            TaskInfo {
                id: 1,
                name: "test:root".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 2,
                name: "test:branch-a".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 3,
                name: "test:branch-b".to_string(),
                show_output: false,
                is_process: false,
            },
            TaskInfo {
                id: 4,
                name: "test:shared".to_string(),
                show_output: false,
                is_process: false,
            },
        ],
        vec![
            (1, 2), // root -> branch-a
            (1, 3), // root -> branch-b
            (2, 4), // branch-a -> shared (primary parent)
            (3, 4), // branch-b -> shared (additional parent)
        ],
    );
    model.apply_activity_event(hierarchy);

    // Start all tasks
    model.apply_activity_event(task_start(1));
    model.apply_activity_event(task_start(2));
    model.apply_activity_event(task_start(3));
    model.apply_activity_event(task_start(4));

    let output = render_to_string(&model, &ui_state);
    // Shared task should appear under both branch-a and branch-b
    insta::assert_snapshot!(output);
}

/// Test that when activities overflow a small terminal, the bottom content
/// (running processes with logs) remains visible and the summary line is last.
#[test]
fn test_overflow_clips_top_keeps_bottom() {
    let model = ActivityModel::new();
    let mut ui_state = UiState::new();
    // Use a very small terminal height to force overflow
    ui_state.set_terminal_size(TEST_WIDTH, 10);

    let mut model = model;

    // Create several completed activities (these should get clipped at the top)
    for i in 1..=5 {
        model.apply_activity_event(build_start(i, format!("completed-build-{}", i)));
        model.apply_activity_event(build_complete(i, ActivityOutcome::Success));
    }

    // Create a running process with logs (this should remain visible at the bottom)
    model.apply_activity_event(process_start_with(
        10,
        "web-server",
        None,
        ActivityLevel::Info,
    ));
    model.apply_activity_event(process_log(10, "Listening on port 3000", false));

    let output = render_to_string(&model, &ui_state);

    // The last non-empty line should be the summary/status line (contains nav hints)
    let lines: Vec<&str> = output.lines().collect();
    let last_non_empty = lines.iter().rev().find(|l| !l.trim().is_empty()).unwrap();
    assert!(
        last_non_empty.contains("nav"),
        "Last line should be the summary status line, got: {:?}",
        last_non_empty
    );

    // The process log should be visible
    assert!(
        output.contains("Listening on port 3000"),
        "Process log line should be visible in overflow output.\nFull output:\n{}",
        output
    );

    // The very last line should be the summary (no trailing empty lines)
    let last_line = lines.last().unwrap();
    assert!(
        !last_line.trim().is_empty(),
        "Last line should be summary, not empty.\nFull output (debug):\n{:?}",
        output
    );
}

#[test]
fn test_hide_stopped_processes_filters_manually_stopped_processes_but_keeps_failures() {
    let (mut model, mut ui_state) = new_test_model();

    model.apply_activity_event(operation_start_with(
        100,
        "Running processes",
        None,
        None,
        ActivityLevel::Info,
    ));

    for (id, name) in [(1, "clean-stop"), (2, "failed-stop"), (3, "running")] {
        model.apply_activity_event(process_start_with(id, name, Some(100), ActivityLevel::Info));
    }

    model.apply_activity_event(process_status(1, devenv_activity::ProcessStatus::Running));
    model.apply_activity_event(process_status(1, devenv_activity::ProcessStatus::Stopped));
    model.apply_activity_event(process_complete(2, ActivityOutcome::Failed));

    let visible_before: Vec<_> = model
        .get_display_activities(&ui_state)
        .into_iter()
        .map(|da| da.activity.name)
        .collect();
    assert!(visible_before.contains(&"clean-stop".to_string()));
    assert!(visible_before.contains(&"failed-stop".to_string()));
    assert!(visible_before.contains(&"running".to_string()));

    ui_state.hide_stopped_processes = true;

    let visible_after: Vec<_> = model
        .get_display_activities(&ui_state)
        .into_iter()
        .map(|da| da.activity.name)
        .collect();
    assert!(!visible_after.contains(&"clean-stop".to_string()));
    assert!(visible_after.contains(&"failed-stop".to_string()));
    assert!(visible_after.contains(&"running".to_string()));

    let summary = model.calculate_summary();
    assert_eq!(
        summary.stopped_processes, 1,
        "summary.stopped_processes must match filter behaviour: only counts processes \
         the filter would actually hide (i.e. clean stops, not failures)"
    );
}

#[test]
fn test_previous_hide_stopped_processes_coverage_used_completed_success() {
    let (mut model, mut ui_state) = new_test_model();

    model.apply_activity_event(operation_start_with(
        100,
        "Running processes",
        None,
        None,
        ActivityLevel::Info,
    ));

    model.apply_activity_event(process_start_with(
        1,
        "clean-stop".to_string(),
        Some(100),
        ActivityLevel::Info,
    ));
    model.apply_activity_event(process_complete(1, ActivityOutcome::Success));

    ui_state.hide_stopped_processes = true;

    let visible_after: Vec<_> = model
        .get_display_activities(&ui_state)
        .into_iter()
        .map(|da| da.activity.name)
        .collect();
    assert!(
        !visible_after.contains(&"clean-stop".to_string()),
        "The old test shape used Process::Complete(Success), which already matched the previous filter."
    );
}

#[test]
fn test_toggle_hide_stopped_processes_clears_hidden_selection() {
    let (mut model, mut ui_state) = new_test_model();

    model.apply_activity_event(operation_start_with(
        100,
        "Running processes",
        None,
        None,
        ActivityLevel::Info,
    ));

    model.apply_activity_event(process_start_with(
        1,
        "clean-stop".to_string(),
        Some(100),
        ActivityLevel::Info,
    ));
    model.apply_activity_event(process_status(1, devenv_activity::ProcessStatus::Stopped));

    ui_state.selected_activity = Some(1);
    ui_state.toggle_hide_stopped_processes();
    if let Some(id) = ui_state.selected_activity
        && !model.is_selectable(id, &ui_state)
    {
        ui_state.selected_activity = None;
    }

    assert!(ui_state.hide_stopped_processes);
    assert_eq!(ui_state.selected_activity, None);
}

#[test]
fn test_hide_stopped_processes_removes_hidden_processes_from_selection() {
    let (mut model, mut ui_state) = new_test_model();

    model.apply_activity_event(operation_start_with(
        100,
        "Running processes",
        None,
        None,
        ActivityLevel::Info,
    ));

    model.apply_activity_event(process_start_with(
        1,
        "clean-stop".to_string(),
        Some(100),
        ActivityLevel::Info,
    ));
    model.apply_activity_event(process_status(1, devenv_activity::ProcessStatus::Stopped));

    model.apply_activity_event(process_start_with(
        2,
        "running".to_string(),
        Some(100),
        ActivityLevel::Info,
    ));

    let selectable_before = model.get_selectable_activity_ids(&ui_state);
    assert_eq!(selectable_before, vec![1, 2]);

    ui_state.hide_stopped_processes = true;

    let selectable_after = model.get_selectable_activity_ids(&ui_state);
    assert_eq!(selectable_after, vec![2]);
}

/// Test cachix push operation starting shows in the TUI.
#[test]
fn test_cachix_push_started() {
    let (mut model, ui_state) = new_test_model();

    model.apply_activity_event(operation_start_with(
        1,
        "Pushing to my-cache",
        None,
        None,
        ActivityLevel::Info,
    ));

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test cachix push operation with progress updates showing path detail.
#[test]
fn test_cachix_push_progress() {
    let (mut model, ui_state) = new_test_model();

    model.apply_activity_event(operation_start_with(
        1,
        "Pushing to my-cache",
        None,
        None,
        ActivityLevel::Info,
    ));

    model.apply_activity_event(operation_progress_with(1, 3, 10, Some("hello-2.12")));

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test cachix push operation completing successfully.
#[test]
fn test_cachix_push_success() {
    let (mut model, ui_state) = new_test_model();

    model.apply_activity_event(operation_start_with(
        1,
        "Pushing to my-cache",
        None,
        None,
        ActivityLevel::Info,
    ));

    model.apply_activity_event(operation_progress(1, 10, 10));

    model.apply_activity_event(operation_complete(1, ActivityOutcome::Success));

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test cachix push operation with failed paths shows error logs.
#[test]
fn test_cachix_push_with_failures() {
    let (mut model, ui_state) = new_test_model();

    model.apply_activity_event(operation_start_with(
        1,
        "Pushing to my-cache",
        None,
        None,
        ActivityLevel::Info,
    ));

    model.apply_activity_event(operation_progress_with(1, 5, 10, Some("openssl-3.0.0")));

    model.apply_activity_event(operation_log(
        1,
        "openssl-3.0.0: HTTP 403: Access Denied",
        true,
    ));

    model.apply_activity_event(operation_log(
        1,
        "glibc-2.37: HTTP 500: Internal Server Error",
        true,
    ));

    model.apply_activity_event(operation_progress(1, 8, 10));

    model.apply_activity_event(operation_complete(1, ActivityOutcome::Failed));

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}

/// Test that non-process activities (like "Evaluating Nix") appear first,
/// followed by processes in alphabetical order.
#[test]
fn test_processes_alphabetical_order() {
    let (mut model, ui_state) = new_test_model();

    // Create the "Running processes" parent operation (matches real usage)
    model.apply_activity_event(operation_start_with(
        100,
        "Running processes",
        None,
        None,
        ActivityLevel::Info,
    ));

    // Start processes in non-alphabetical order (z, a, m)
    model.apply_activity_event(process_start_with(
        1,
        "zookeeper".to_string(),
        Some(100),
        ActivityLevel::Info,
    ));
    model.apply_activity_event(process_start_with(
        2,
        "api-server".to_string(),
        Some(100),
        ActivityLevel::Info,
    ));

    // Add "Evaluating Nix" as a child of "Running processes" (happens during process manager eval)
    model.apply_activity_event(evaluate_start_with(
        3,
        "Evaluating Nix",
        ActivityLevel::Info,
        Some(100),
    ));

    model.apply_activity_event(process_start_with(
        4,
        "mysql".to_string(),
        Some(100),
        ActivityLevel::Info,
    ));

    let output = render_to_string(&model, &ui_state);

    // Verify "Evaluating Nix" comes first, then processes alphabetically
    let eval_pos = output
        .find("Evaluating Nix")
        .expect("Evaluating Nix should be in output");
    let api_pos = output
        .find("api-server")
        .expect("api-server should be in output");
    let mysql_pos = output.find("mysql").expect("mysql should be in output");
    let zoo_pos = output
        .find("zookeeper")
        .expect("zookeeper should be in output");
    assert!(
        eval_pos < api_pos && api_pos < mysql_pos && mysql_pos < zoo_pos,
        "Evaluating Nix should come first, then processes in alphabetical order.\nFull output:\n{}",
        output
    );

    insta::assert_snapshot!(output);
}

/// Test cachix push alongside other activities (build + push concurrent).
#[test]
fn test_cachix_push_alongside_build() {
    let (mut model, ui_state) = new_test_model();

    model.apply_activity_event(build_start(1, "hello-2.12"));
    model.apply_activity_event(build_phase(1, "buildPhase"));

    model.apply_activity_event(operation_start_with(
        2,
        "Pushing to my-cache",
        None,
        None,
        ActivityLevel::Info,
    ));

    model.apply_activity_event(operation_progress_with(2, 3, 7, Some("python-3.11.5")));

    let output = render_to_string(&model, &ui_state);
    insta::assert_snapshot!(output);
}
