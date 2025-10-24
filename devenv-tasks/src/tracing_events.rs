//! Structured tracing events for task execution
//!
//! This module provides helper functions to emit consistent tracing events
//! that can be captured by devenv-tui's tracing layer for real-time display.
//!
//! Uses the standardized tracing interface defined in devenv_tui::tracing_interface

use tracing::{debug, error, info, info_span, warn};

// Import the standardized tracing interface constants
// Note: We use string literals directly to avoid the devenv-tui dependency in devenv-tasks
const OPERATION_TYPE: &str = "operation.type";
const OPERATION_NAME: &str = "operation.name";
const OPERATION_SHORT_NAME: &str = "operation.short_name";
const STATUS: &str = "status";
const PROGRESS_TYPE: &str = "progress.type";
#[allow(dead_code)]
const PROGRESS_CURRENT: &str = "progress.current";
#[allow(dead_code)]
const PROGRESS_TOTAL: &str = "progress.total";

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
