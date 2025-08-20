//! Structured tracing events for task execution
//!
//! This module provides helper functions to emit consistent tracing events
//! that can be captured by devenv-tui's tracing layer for real-time display.

use tracing::{debug, error, info, warn};

/// Emit a structured tracing event when a task starts
pub fn emit_task_start(task_name: &str) {
    info!(
        target: "devenv_tasks",
        task_name = %task_name,
        tui.op = true,
        "Task starting"
    );
}

/// Emit a structured tracing event for task status changes
pub fn emit_task_status_change(task_name: &str, status: &str, result: Option<&str>) {
    info!(
        target: "devenv_tasks",
        task_name = %task_name,
        status = %status,
        ?result,
        tui.op = true,
        "Task status updated"
    );
}

/// Emit a debug event for command execution
pub fn emit_command_start(task_name: &str, command: &str) {
    debug!(
        target: "devenv_tasks",
        task_name = %task_name,
        command = %command,
        tui.log = true,
        "Executing command"
    );
}

/// Emit a debug event for command completion with exit status
pub fn emit_command_end(task_name: &str, command: &str, exit_code: Option<i32>, success: bool) {
    if success {
        debug!(
            target: "devenv_tasks",
            task_name = %task_name,
            command = %command,
            exit_code = ?exit_code,
            tui.log = true,
            "Command completed successfully"
        );
    } else {
        warn!(
            target: "devenv_tasks",
            task_name = %task_name,
            command = %command,
            exit_code = ?exit_code,
            tui.log = true,
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
    match result {
        "success" => {
            info!(
                target: "devenv_tasks",
                task_name = %task_name,
                status = %status,
                result = %result,
                ?duration_secs,
                ?reason,
                tui.op = true,
                "Task completed successfully"
            );
        }
        "failed" => {
            error!(
                target: "devenv_tasks",
                task_name = %task_name,
                status = %status,
                result = %result,
                ?duration_secs,
                ?reason,
                tui.op = true,
                "Task failed"
            );
        }
        "cached" => {
            info!(
                target: "devenv_tasks",
                task_name = %task_name,
                status = %status,
                result = %result,
                ?duration_secs,
                ?reason,
                tui.op = true,
                "Task skipped (cached)"
            );
        }
        "skipped" => {
            info!(
                target: "devenv_tasks",
                task_name = %task_name,
                status = %status,
                result = %result,
                ?duration_secs,
                ?reason,
                tui.op = true,
                "Task skipped"
            );
        }
        "dependency_failed" => {
            warn!(
                target: "devenv_tasks",
                task_name = %task_name,
                status = %status,
                result = %result,
                ?duration_secs,
                ?reason,
                tui.op = true,
                "Task skipped due to dependency failure"
            );
        }
        "cancelled" => {
            warn!(
                target: "devenv_tasks",
                task_name = %task_name,
                status = %status,
                result = %result,
                ?duration_secs,
                ?reason,
                tui.op = true,
                "Task cancelled"
            );
        }
        _ => {
            info!(
                target: "devenv_tasks",
                task_name = %task_name,
                status = %status,
                result = %result,
                ?duration_secs,
                ?reason,
                tui.op = true,
                "Task completed"
            );
        }
    }
}
