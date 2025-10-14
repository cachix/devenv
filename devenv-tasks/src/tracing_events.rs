//! Structured tracing events for task execution
//!
//! This module provides helper functions to emit consistent tracing events
//! that can be captured by devenv-tui's tracing layer for real-time display.

use tracing::{debug, error, info, warn};

/// Emit a structured tracing event when a task starts
pub fn emit_task_start(task_name: &str) {
    info!(
        target: "devenv.ui",
        task_name,
        devenv.ui.message = task_name,
        devenv.ui.type = "task",
        devenv.ui.detail = "starting",
        devenv.ui.id = format!("task-{}", task_name),
        "Task starting"
    );
}

/// Emit a structured tracing event for task status changes
pub fn emit_task_status_change(task_name: &str, status: &str, result: Option<&str>) {
    info!(
        target: "devenv.ui.progress",
        task_name,
        status,
        ?result,
        devenv.ui.id = format!("task-{}", task_name),
        devenv.ui.detail = status,
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
    let target = "devenv.ui.progress";
    let id = format!("task-{task_name}");
    match result {
        "success" => {
            info!(
                target,
                task_name,
                status,
                result,
                ?duration_secs,
                ?reason,
                devenv.ui.id = id,
                devenv.ui.detail = "completed successfully",
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
                devenv.ui.id = id,
                devenv.ui.detail = "failed",
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
                devenv.ui.id = id,
                devenv.ui.detail = "skipped (cached)",
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
                devenv.ui.id = id,
                devenv.ui.detail = "skipped",
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
                devenv.ui.id = id,
                devenv.ui.detail = "skipped due to dependency failure",
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
                devenv.ui.id = %id,
                devenv.ui.detail = "cancelled",
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
                devenv.ui.id = %id,
                devenv.ui.detail = "completed",
                "Task completed"
            );
        }
    }
}
