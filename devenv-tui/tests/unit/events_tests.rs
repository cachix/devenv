use devenv_tui::{
    BuildLogUpdate, BuildPhaseUpdate, DownloadProgressUpdate, EvaluationProgressUpdate,
    LogOutputUpdate, NixProgressUpdate, TaskStatusUpdate, TracingUpdate,
};

use crate::test_utils::builders::create_test_fields;

#[test]
fn test_parse_nix_progress_update_complete() {
    let fields = create_test_fields(&[
        ("activity_id", "42"),
        ("done", "10"),
        ("expected", "100"),
        ("running", "2"),
        ("failed", "1"),
    ]);

    let update = NixProgressUpdate::from_fields(&fields);
    assert!(update.is_some());

    let update = update.unwrap();
    assert_eq!(update.activity_id, 42);
    assert_eq!(update.done, 10);
    assert_eq!(update.expected, 100);
    assert_eq!(update.running, 2);
    assert_eq!(update.failed, 1);
}

#[test]
fn test_parse_nix_progress_update_minimal() {
    let fields = create_test_fields(&[
        ("activity_id", "42"),
        ("done", "10"),
        ("expected", "100"),
    ]);

    let update = NixProgressUpdate::from_fields(&fields);
    assert!(update.is_some());

    let update = update.unwrap();
    assert_eq!(update.activity_id, 42);
    assert_eq!(update.done, 10);
    assert_eq!(update.expected, 100);
    assert_eq!(update.running, 0);
    assert_eq!(update.failed, 0);
}

#[test]
fn test_parse_nix_progress_update_missing_required() {
    let fields = create_test_fields(&[("activity_id", "42"), ("done", "10")]);

    let update = NixProgressUpdate::from_fields(&fields);
    assert!(update.is_none());
}

#[test]
fn test_parse_build_phase_update() {
    let fields = create_test_fields(&[("activity_id", "42"), ("phase", "buildPhase")]);

    let update = BuildPhaseUpdate::from_fields(&fields);
    assert!(update.is_some());

    let update = update.unwrap();
    assert_eq!(update.activity_id, 42);
    assert_eq!(update.phase, "buildPhase");
}

#[test]
fn test_parse_build_log_update() {
    let fields = create_test_fields(&[
        ("activity_id", "42"),
        ("line", "Building package..."),
    ]);

    let update = BuildLogUpdate::from_fields(&fields);
    assert!(update.is_some());

    let update = update.unwrap();
    assert_eq!(update.activity_id, 42);
    assert_eq!(update.line, "Building package...");
}

#[test]
fn test_parse_download_progress_update() {
    let fields = create_test_fields(&[
        ("activity_id", "42"),
        ("bytes_downloaded", "1024"),
        ("total_bytes", "2048"),
    ]);

    let update = DownloadProgressUpdate::from_fields(&fields);
    assert!(update.is_some());

    let update = update.unwrap();
    assert_eq!(update.activity_id, 42);
    assert_eq!(update.bytes_downloaded, 1024);
    assert_eq!(update.total_bytes, Some(2048));
}

#[test]
fn test_parse_download_progress_update_without_total() {
    let fields = create_test_fields(&[("activity_id", "42"), ("bytes_downloaded", "1024")]);

    let update = DownloadProgressUpdate::from_fields(&fields);
    assert!(update.is_some());

    let update = update.unwrap();
    assert_eq!(update.activity_id, 42);
    assert_eq!(update.bytes_downloaded, 1024);
    assert_eq!(update.total_bytes, None);
}

#[test]
fn test_parse_evaluation_progress_update() {
    let fields = create_test_fields(&[
        ("activity_id", "42"),
        ("total_files_evaluated", "100"),
        ("files", r#"["file1.nix", "file2.nix"]"#),
    ]);

    let update = EvaluationProgressUpdate::from_fields(&fields);
    assert!(update.is_some());

    let update = update.unwrap();
    assert_eq!(update.activity_id, 42);
    assert_eq!(update.total_files_evaluated, 100);
    assert_eq!(update.latest_files.len(), 2);
    assert_eq!(update.latest_files[0], "file1.nix");
    assert_eq!(update.latest_files[1], "file2.nix");
}

#[test]
fn test_parse_evaluation_progress_update_minimal() {
    let fields = create_test_fields(&[("activity_id", "42")]);

    let update = EvaluationProgressUpdate::from_fields(&fields);
    assert!(update.is_some());

    let update = update.unwrap();
    assert_eq!(update.activity_id, 42);
    assert_eq!(update.total_files_evaluated, 0);
    assert_eq!(update.latest_files.len(), 0);
}

#[test]
fn test_parse_task_status_update() {
    let fields = create_test_fields(&[
        ("name", "test-task"),
        ("status", "running"),
        ("duration_secs", "30.5"),
    ]);

    let update = TaskStatusUpdate::from_fields(&fields);
    assert!(update.is_some());

    let update = update.unwrap();
    assert_eq!(update.name, "test-task");
    assert_eq!(update.status, "running");
    assert_eq!(update.duration_secs, Some(30.5));
}

#[test]
fn test_parse_task_status_update_with_success() {
    let fields = create_test_fields(&[
        ("name", "test-task"),
        ("status", "completed"),
        ("success", "true"),
        ("result", "passed"),
    ]);

    let update = TaskStatusUpdate::from_fields(&fields);
    assert!(update.is_some());

    let update = update.unwrap();
    assert_eq!(update.name, "test-task");
    assert_eq!(update.status, "completed");
    assert_eq!(update.success, Some(true));
    assert_eq!(update.result, Some("passed".to_string()));
}

#[test]
fn test_parse_task_status_update_with_error() {
    let fields = create_test_fields(&[
        ("name", "test-task"),
        ("status", "failed"),
        ("success", "false"),
        ("error", "Build error occurred"),
    ]);

    let update = TaskStatusUpdate::from_fields(&fields);
    assert!(update.is_some());

    let update = update.unwrap();
    assert_eq!(update.name, "test-task");
    assert_eq!(update.status, "failed");
    assert_eq!(update.success, Some(false));
    assert_eq!(update.error, Some("Build error occurred".to_string()));
}

#[test]
fn test_parse_log_output_update() {
    let fields = create_test_fields(&[
        ("stream", "stdout"),
        ("message", "Log message content"),
    ]);

    let update = LogOutputUpdate::from_fields(&fields);
    assert!(update.is_some());

    let update = update.unwrap();
    assert_eq!(update.stream, "stdout");
    assert_eq!(update.message, "Log message content");
}

#[test]
fn test_tracing_update_from_event_nix_progress() {
    let fields = create_test_fields(&[
        ("activity_id", "42"),
        ("done", "10"),
        ("expected", "100"),
    ]);

    let update = TracingUpdate::from_event("devenv.nix.progress", "progress", &fields);
    assert!(update.is_some());

    match update.unwrap() {
        TracingUpdate::NixProgress(progress) => {
            assert_eq!(progress.activity_id, 42);
            assert_eq!(progress.done, 10);
            assert_eq!(progress.expected, 100);
        }
        _ => panic!("Expected NixProgress variant"),
    }
}

#[test]
fn test_tracing_update_from_event_build_phase() {
    let fields = create_test_fields(&[("activity_id", "42"), ("phase", "buildPhase")]);

    let update = TracingUpdate::from_event("devenv.nix.build", "build", &fields);
    assert!(update.is_some());

    match update.unwrap() {
        TracingUpdate::BuildPhase(phase) => {
            assert_eq!(phase.activity_id, 42);
            assert_eq!(phase.phase, "buildPhase");
        }
        _ => panic!("Expected BuildPhase variant"),
    }
}

#[test]
fn test_tracing_update_from_event_build_log() {
    let fields = create_test_fields(&[("activity_id", "42"), ("line", "Building...")]);

    let update = TracingUpdate::from_event("devenv.nix.build", "log", &fields);
    assert!(update.is_some());

    match update.unwrap() {
        TracingUpdate::BuildLog(log) => {
            assert_eq!(log.activity_id, 42);
            assert_eq!(log.line, "Building...");
        }
        _ => panic!("Expected BuildLog variant"),
    }
}

#[test]
fn test_tracing_update_from_event_download_progress() {
    let fields = create_test_fields(&[("activity_id", "42"), ("bytes_downloaded", "1024")]);

    let update = TracingUpdate::from_event("devenv.nix.download", "nix_download_progress", &fields);
    assert!(update.is_some());

    match update.unwrap() {
        TracingUpdate::DownloadProgress(download) => {
            assert_eq!(download.activity_id, 42);
            assert_eq!(download.bytes_downloaded, 1024);
        }
        _ => panic!("Expected DownloadProgress variant"),
    }
}

#[test]
fn test_tracing_update_from_event_evaluation_progress() {
    let fields = create_test_fields(&[("activity_id", "42"), ("total_files_evaluated", "50")]);

    let update =
        TracingUpdate::from_event("devenv.nix.eval", "nix_evaluation_progress", &fields);
    assert!(update.is_some());

    match update.unwrap() {
        TracingUpdate::EvaluationProgress(eval) => {
            assert_eq!(eval.activity_id, 42);
            assert_eq!(eval.total_files_evaluated, 50);
        }
        _ => panic!("Expected EvaluationProgress variant"),
    }
}

#[test]
fn test_tracing_update_from_event_task_status() {
    let fields = create_test_fields(&[("name", "test-task"), ("status", "running")]);

    let update = TracingUpdate::from_event("devenv_tasks::runner", "task_update", &fields);
    assert!(update.is_some());

    match update.unwrap() {
        TracingUpdate::TaskStatus(task) => {
            assert_eq!(task.name, "test-task");
            assert_eq!(task.status, "running");
        }
        _ => panic!("Expected TaskStatus variant"),
    }
}

#[test]
fn test_tracing_update_from_event_log_output_stdout() {
    let fields = create_test_fields(&[("stream", "stdout"), ("message", "Output line")]);

    let update = TracingUpdate::from_event("stdout", "log", &fields);
    assert!(update.is_some());

    match update.unwrap() {
        TracingUpdate::LogOutput(log) => {
            assert_eq!(log.stream, "stdout");
            assert_eq!(log.message, "Output line");
        }
        _ => panic!("Expected LogOutput variant"),
    }
}

#[test]
fn test_tracing_update_from_event_log_output_stderr() {
    let fields = create_test_fields(&[("stream", "stderr"), ("message", "Error line")]);

    let update = TracingUpdate::from_event("stderr", "error", &fields);
    assert!(update.is_some());

    match update.unwrap() {
        TracingUpdate::LogOutput(log) => {
            assert_eq!(log.stream, "stderr");
            assert_eq!(log.message, "Error line");
        }
        _ => panic!("Expected LogOutput variant"),
    }
}

#[test]
fn test_tracing_update_from_event_unknown_target() {
    let fields = create_test_fields(&[("key", "value")]);

    let update = TracingUpdate::from_event("unknown.target", "event", &fields);
    assert!(update.is_none());
}

#[test]
fn test_tracing_update_from_event_malformed_data() {
    let fields = create_test_fields(&[("invalid", "data")]);

    let update = TracingUpdate::from_event("devenv.nix.progress", "progress", &fields);
    assert!(update.is_none());
}
