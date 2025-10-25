//! Structured tracing events for task execution
//!
//! This module provides helper functions to emit consistent tracing events
//! that can be captured by devenv-tui's tracing layer for real-time display.
//!
//! Uses the standardized tracing interface defined in devenv_tui::tracing_interface

use devenv_tui::tracing_interface::{operation_fields, progress_events, status_events};
use tracing::{debug, error, info, info_span, warn};

// Re-export commonly used constants for convenience
use operation_fields::{
    NAME as OPERATION_NAME, SHORT_NAME as OPERATION_SHORT_NAME, TYPE as OPERATION_TYPE,
};
use progress_events::fields::{
    CURRENT as PROGRESS_CURRENT, TOTAL as PROGRESS_TOTAL, TYPE as PROGRESS_TYPE,
};
use status_events::fields::STATUS;

/// Emit a structured tracing event when a task starts
pub fn emit_task_start(task_name: &str) {
    let _span = info_span!(
        "task_start",
        { OPERATION_TYPE } = "task",
        { OPERATION_NAME } = task_name,
        { OPERATION_SHORT_NAME } = task_name,
        task_name = task_name,
    )
    .entered();

    info!({ STATUS } = "starting", "Task starting");
}

/// Emit a structured tracing event for task status changes
pub fn emit_task_status_change(task_name: &str, status: &str, result: Option<&str>) {
    info!(
        target: "devenv_tasks",
        task_name,
        status,
        ?result,
        { STATUS } = status,
        "Task status updated"
    );
}

/// Emit a debug event for command execution
pub fn emit_command_start(task_name: &str, command: &str) {
    debug!(
        target: "devenv.tasks",
        task_name,
        command,
        devenv.log = true,
        "Executing command"
    );
}

/// Emit a debug event for command completion with exit status
pub fn emit_command_end(task_name: &str, command: &str, exit_code: Option<i32>, success: bool) {
    if success {
        debug!(
            target: "devenv.tasks",
            task_name,
            command,
            ?exit_code,
            devenv.log = true,
            "Command completed successfully"
        );
    } else {
        warn!(
            target: "devenv.tasks",
            task_name,
            command,
            ?exit_code,
            devenv.log = true,
            "Command failed"
        );
    }
}

/// Emit comprehensive tracing events for task completion
pub fn emit_task_completed(
    task_name: &str,
    status: &str,
    result: &str,
    duration_secs: Option<f64>,
    reason: Option<&str>,
) {
    let target = "devenv_tasks";
    match result {
        "success" => {
            info!(
                target,
                task_name,
                status,
                result,
                ?duration_secs,
                ?reason,
                { STATUS } = "completed",
                "Task completed successfully"
            );
        }
        "failed" => {
            error!(
                target,
                task_name,
                status,
                result,
                ?duration_secs,
                ?reason,
                { STATUS } = "failed",
                "Task failed"
            );
        }
        "cached" => {
            info!(
                target,
                task_name,
                status,
                result,
                ?duration_secs,
                ?reason,
                { STATUS } = "completed",
                { PROGRESS_TYPE } = "indeterminate",
                "Task skipped (cached)"
            );
        }
        "skipped" => {
            info!(
                target,
                task_name,
                status,
                result,
                ?duration_secs,
                ?reason,
                { STATUS } = "completed",
                "Task skipped"
            );
        }
        "dependency_failed" => {
            warn!(
                target,
                task_name,
                status,
                result,
                ?duration_secs,
                ?reason,
                { STATUS } = "failed",
                "Task skipped due to dependency failure"
            );
        }
        "cancelled" => {
            warn!(
                target,
                task_name,
                status,
                result,
                ?duration_secs,
                ?reason,
                { STATUS } = "cancelled",
                "Task cancelled"
            );
        }
        _ => {
            info!(
                target,
                task_name,
                status,
                result,
                ?duration_secs,
                ?reason,
                { STATUS } = "completed",
                "Task completed"
            );
        }
    }
}

// Note: Task spans should be created using devenv_tui::tracing_interface::create_task_span
// Note: stdout/stderr events should be emitted directly in task_state.rs
// where the parent task span is available. This allows proper parent-child
// association using `parent: &span` in the event macro.
//
// Example usage:
// ```
// event!(
//     target: build_log_events::STDOUT_TARGET,
//     parent: &task_span,
//     Level::INFO,
//     {build_log_events::fields::STREAM} = "stdout",
//     {build_log_events::fields::MESSAGE} = %line,
// );
// ```

/// Emit a progress event
///
/// This allows tasks to report progress for display in the TUI.
/// Progress can be based on counts, bytes, percentages, or be indeterminate.
#[allow(dead_code)]
pub fn emit_progress(
    progress_type: &str,
    current: Option<u64>,
    total: Option<u64>,
    rate: Option<f64>,
) {
    match (current, total, rate) {
        (Some(c), Some(t), Some(r)) => {
            info!(
                { PROGRESS_TYPE } = progress_type,
                { PROGRESS_CURRENT } = c,
                { PROGRESS_TOTAL } = t,
                { progress_events::fields::RATE } = r,
                "Progress update"
            );
        }
        (Some(c), Some(t), None) => {
            info!(
                { PROGRESS_TYPE } = progress_type,
                { PROGRESS_CURRENT } = c,
                { PROGRESS_TOTAL } = t,
                "Progress update"
            );
        }
        (Some(c), None, _) => {
            info!(
                { PROGRESS_TYPE } = progress_type,
                { PROGRESS_CURRENT } = c,
                "Progress update"
            );
        }
        (None, _, _) => {
            info!({ PROGRESS_TYPE } = progress_type, "Progress update");
        }
    }
}
