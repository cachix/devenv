//! Marker-based task execution in PTY.
//!
//! This module provides functionality to execute tasks inside a PTY shell
//! using a marker-based protocol to delimit command output.

use crate::protocol::{PtyTaskRequest, PtyTaskResult};
use crate::pty::Pty;
use regex::Regex;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use tokio::sync::mpsc;

/// Errors that can occur during task execution.
#[derive(Debug, Error)]
pub enum TaskRunnerError {
    #[error("PTY error: {0}")]
    Pty(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("shell closed before ready")]
    ShellNotReady,
    #[error("spawn_blocking failed: {0}")]
    SpawnBlocking(String),
}

/// Strip ANSI escape sequences from a string.
///
/// Used for marker detection in PTY output where ANSI codes can appear
/// around markers due to terminal control sequences.
pub fn strip_ansi_codes(s: &str) -> String {
    // Match ANSI escape sequences:
    // - CSI sequences: ESC [ followed by parameters (digits, semicolons, ?, >) ending with a letter
    //   Example: \x1b[?2004l (bracketed paste mode), \x1b[0m (reset), \x1b[1;32m (color)
    // - OSC sequences: ESC ] ... BEL
    // - Character set selection: ESC ( or ESC ) followed by character
    static ANSI_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = ANSI_RE.get_or_init(|| {
        Regex::new(r"\x1b\[[0-9;?>=]*[a-zA-Z]|\x1b\][^\x07]*\x07|\x1b[()][0-9A-Z]").unwrap()
    });
    re.replace_all(s, "").to_string()
}

/// Marker-based task execution in PTY.
///
/// Uses echo markers to delimit command output:
/// ```text
/// echo '__DEVENV_TASK_START_<id>__'
/// <command>
/// echo '__DEVENV_TASK_END_<id>_'$?'__'
/// ```
pub struct PtyTaskRunner {
    pty: Arc<Pty>,
}

impl PtyTaskRunner {
    /// Create a new task runner for the given PTY.
    pub fn new(pty: Arc<Pty>) -> Self {
        Self { pty }
    }

    /// Wait for shell to signal readiness.
    ///
    /// Looks for `__DEVENV_SHELL_READY__` marker in PTY output.
    /// This should be emitted by the shell's rcfile after initialization.
    pub async fn wait_for_shell_ready(&self) -> Result<(), TaskRunnerError> {
        tracing::trace!("wait_for_shell_ready: waiting for shell to be ready");

        let mut init_buffer = String::new();
        loop {
            let pty_clone = Arc::clone(&self.pty);
            let read_result = tokio::task::spawn_blocking(move || {
                let mut buf = [0u8; 4096];
                match pty_clone.read(&mut buf) {
                    Ok(n) => Ok((buf, n)),
                    Err(e) => Err(e),
                }
            })
            .await;

            let (buf, n) = match read_result {
                Ok(Ok((buf, n))) => (buf, n),
                Ok(Err(e)) => {
                    tracing::error!("wait_for_shell_ready: PTY read error during init: {}", e);
                    return Err(TaskRunnerError::Io(e));
                }
                Err(e) => {
                    tracing::error!("wait_for_shell_ready: spawn_blocking failed: {}", e);
                    return Err(TaskRunnerError::SpawnBlocking(e.to_string()));
                }
            };

            if n == 0 {
                tracing::error!("wait_for_shell_ready: PTY closed before shell ready");
                return Err(TaskRunnerError::ShellNotReady);
            }

            let chunk = String::from_utf8_lossy(&buf[..n]);
            init_buffer.push_str(&chunk);
            tracing::trace!("wait_for_shell_ready: init buffer: {:?}", init_buffer);

            if init_buffer.contains("__DEVENV_SHELL_READY__") {
                tracing::trace!("wait_for_shell_ready: shell is ready");
                return Ok(());
            }
        }
    }

    /// Execute a single task request and send result via response channel.
    pub async fn execute(&self, request: PtyTaskRequest) {
        let id = request.id;
        let start_marker = format!("__DEVENV_TASK_START_{id}__");
        let end_marker_prefix = format!("__DEVENV_TASK_END_{id}_");

        tracing::trace!("execute: task id={}, cmd={}", request.id, request.command);

        // Build the command to execute with markers
        let mut cmd_parts = Vec::new();

        // Echo start marker
        cmd_parts.push(format!("echo '{start_marker}'"));

        // Set environment variables
        for (key, value) in &request.env {
            let escaped = value.replace('\'', "'\\''");
            cmd_parts.push(format!("export {key}='{escaped}'"));
        }

        // Change directory if specified
        if let Some(ref cwd) = request.cwd {
            cmd_parts.push(format!("cd '{cwd}'"));
        }

        // Execute the command
        cmd_parts.push(request.command.clone());

        // Echo end marker with exit code
        cmd_parts.push(format!("echo '{end_marker_prefix}'$?'__'"));

        // Join with semicolons and add newline
        let full_cmd = format!("{}\n", cmd_parts.join("; "));

        // Write command to PTY
        tracing::trace!("execute: writing command to PTY:\n{}", full_cmd);
        if let Err(e) = self.pty.write_all(full_cmd.as_bytes()) {
            let _ = request.response_tx.send(PtyTaskResult {
                success: false,
                stdout_lines: Vec::new(),
                stderr_lines: Vec::new(),
                error: Some(format!("Failed to write to PTY: {}", e)),
            });
            return;
        }
        if let Err(e) = self.pty.flush() {
            let _ = request.response_tx.send(PtyTaskResult {
                success: false,
                stdout_lines: Vec::new(),
                stderr_lines: Vec::new(),
                error: Some(format!("Failed to flush PTY: {}", e)),
            });
            return;
        }

        // Read PTY output until we see the end marker
        let mut output_buffer = String::new();
        let mut stdout_lines = Vec::new();
        let mut started = false;
        let mut error_msg: Option<String> = None;
        let mut exit_code: Option<i32> = None;

        'read_loop: loop {
            let pty_clone = Arc::clone(&self.pty);
            let read_result = tokio::task::spawn_blocking(move || {
                let mut buf = [0u8; 4096];
                match pty_clone.read(&mut buf) {
                    Ok(n) => Ok((buf, n)),
                    Err(e) => Err(e),
                }
            })
            .await;

            let (buf, n) = match read_result {
                Ok(Ok((buf, n))) => (buf, n),
                Ok(Err(e)) => {
                    error_msg = Some(format!("PTY read error: {e}"));
                    break 'read_loop;
                }
                Err(e) => {
                    error_msg = Some(format!("spawn_blocking failed: {e}"));
                    break 'read_loop;
                }
            };

            if n == 0 {
                tracing::trace!("execute: PTY returned 0 bytes (closed)");
                error_msg = Some("PTY closed unexpectedly".to_string());
                break 'read_loop;
            }

            let chunk = String::from_utf8_lossy(&buf[..n]);
            tracing::trace!("execute: read {} bytes: {:?}", n, chunk);
            output_buffer.push_str(&chunk);

            // Process complete lines
            while let Some(newline_pos) = output_buffer.find('\n') {
                let line = output_buffer[..newline_pos].to_string();
                output_buffer = output_buffer[newline_pos + 1..].to_string();

                // Strip ANSI codes and trim whitespace for marker detection
                let clean = strip_ansi_codes(&line);
                let trimmed = clean.trim();

                tracing::trace!("execute: line (started={}): {:?}", started, trimmed);

                // Check for start marker - must be exact match
                if !started && trimmed == start_marker {
                    tracing::trace!("execute: found start marker");
                    started = true;
                    continue;
                }

                // Check for end marker
                if started && trimmed.starts_with(&end_marker_prefix) && trimmed.ends_with("__") {
                    exit_code = trimmed
                        .strip_prefix(&end_marker_prefix)
                        .and_then(|s| s.strip_suffix("__"))
                        .and_then(|s| s.parse::<i32>().ok());
                    tracing::trace!("execute: found end marker, exit_code={:?}", exit_code);
                    break 'read_loop;
                }

                // Capture output if task has started
                if started {
                    stdout_lines.push((Instant::now(), line));
                }
            }
        }

        // Build result
        let (success, error) = if let Some(err) = error_msg {
            (false, Some(err))
        } else {
            let code = exit_code.unwrap_or(1);
            (
                code == 0,
                if code == 0 {
                    None
                } else {
                    Some(format!("Task exited with code {code}"))
                },
            )
        };

        tracing::trace!("execute: result success={}, error={:?}", success, error);

        let _ = request.response_tx.send(PtyTaskResult {
            success,
            stdout_lines,
            stderr_lines: Vec::new(),
            error,
        });
    }

    /// Run task loop, processing requests until channel closes.
    pub async fn run_loop(
        &self,
        task_rx: &mut mpsc::Receiver<PtyTaskRequest>,
    ) -> Result<(), TaskRunnerError> {
        tracing::trace!("run_loop: waiting for task requests");

        while let Some(request) = task_rx.recv().await {
            self.execute(request).await;
        }

        tracing::trace!("run_loop: task channel closed, exiting");
        Ok(())
    }

    /// Run task loop with VT state tracking.
    ///
    /// This is the main entry point for running tasks in the PTY before
    /// terminal handoff. It waits for shell readiness, then processes
    /// task requests until the channel closes.
    pub async fn run_with_vt(
        &self,
        task_rx: &mut mpsc::Receiver<PtyTaskRequest>,
        vt: &mut avt::Vt,
    ) -> Result<(), TaskRunnerError> {
        // Wait for shell to be ready
        self.wait_for_shell_ready_with_vt(vt).await?;

        tracing::trace!("run_with_vt: waiting for task requests");

        while let Some(request) = task_rx.recv().await {
            self.execute_with_vt(request, vt).await;
        }

        tracing::trace!("run_with_vt: task channel closed, exiting");
        Ok(())
    }

    /// Wait for shell ready while feeding VT.
    async fn wait_for_shell_ready_with_vt(&self, vt: &mut avt::Vt) -> Result<(), TaskRunnerError> {
        tracing::trace!("wait_for_shell_ready_with_vt: waiting for shell to be ready");

        let mut init_buffer = String::new();
        loop {
            let pty_clone = Arc::clone(&self.pty);
            let read_result = tokio::task::spawn_blocking(move || {
                let mut buf = [0u8; 4096];
                match pty_clone.read(&mut buf) {
                    Ok(n) => Ok((buf, n)),
                    Err(e) => Err(e),
                }
            })
            .await;

            let (buf, n) = match read_result {
                Ok(Ok((buf, n))) => (buf, n),
                Ok(Err(e)) => {
                    tracing::error!("wait_for_shell_ready_with_vt: PTY read error: {}", e);
                    return Err(TaskRunnerError::Io(e));
                }
                Err(e) => {
                    tracing::error!("wait_for_shell_ready_with_vt: spawn_blocking failed: {}", e);
                    return Err(TaskRunnerError::SpawnBlocking(e.to_string()));
                }
            };

            if n == 0 {
                tracing::error!("wait_for_shell_ready_with_vt: PTY closed before shell ready");
                return Err(TaskRunnerError::ShellNotReady);
            }

            let chunk = String::from_utf8_lossy(&buf[..n]);
            vt.feed_str(&chunk);
            init_buffer.push_str(&chunk);
            tracing::trace!(
                "wait_for_shell_ready_with_vt: init buffer: {:?}",
                init_buffer
            );

            if init_buffer.contains("__DEVENV_SHELL_READY__") {
                tracing::trace!("wait_for_shell_ready_with_vt: shell is ready");
                return Ok(());
            }
        }
    }

    /// Execute a task while feeding VT.
    async fn execute_with_vt(&self, request: PtyTaskRequest, vt: &mut avt::Vt) {
        let id = request.id;
        let start_marker = format!("__DEVENV_TASK_START_{id}__");
        let end_marker_prefix = format!("__DEVENV_TASK_END_{id}_");

        tracing::trace!(
            "execute_with_vt: task id={}, cmd={}",
            request.id,
            request.command
        );

        // Build the command to execute with markers
        let mut cmd_parts = Vec::new();
        cmd_parts.push(format!("echo '{start_marker}'"));

        for (key, value) in &request.env {
            let escaped = value.replace('\'', "'\\''");
            cmd_parts.push(format!("export {key}='{escaped}'"));
        }

        if let Some(ref cwd) = request.cwd {
            cmd_parts.push(format!("cd '{cwd}'"));
        }

        cmd_parts.push(request.command.clone());
        cmd_parts.push(format!("echo '{end_marker_prefix}'$?'__'"));

        let full_cmd = format!("{}\n", cmd_parts.join("; "));

        tracing::trace!("execute_with_vt: writing command to PTY:\n{}", full_cmd);
        if let Err(e) = self.pty.write_all(full_cmd.as_bytes()) {
            let _ = request.response_tx.send(PtyTaskResult {
                success: false,
                stdout_lines: Vec::new(),
                stderr_lines: Vec::new(),
                error: Some(format!("Failed to write to PTY: {}", e)),
            });
            return;
        }
        if let Err(e) = self.pty.flush() {
            let _ = request.response_tx.send(PtyTaskResult {
                success: false,
                stdout_lines: Vec::new(),
                stderr_lines: Vec::new(),
                error: Some(format!("Failed to flush PTY: {}", e)),
            });
            return;
        }

        let mut output_buffer = String::new();
        let mut stdout_lines = Vec::new();
        let mut started = false;
        let mut error_msg: Option<String> = None;
        let mut exit_code: Option<i32> = None;

        'read_loop: loop {
            let pty_clone = Arc::clone(&self.pty);
            let read_result = tokio::task::spawn_blocking(move || {
                let mut buf = [0u8; 4096];
                match pty_clone.read(&mut buf) {
                    Ok(n) => Ok((buf, n)),
                    Err(e) => Err(e),
                }
            })
            .await;

            let (buf, n) = match read_result {
                Ok(Ok((buf, n))) => (buf, n),
                Ok(Err(e)) => {
                    error_msg = Some(format!("PTY read error: {e}"));
                    break 'read_loop;
                }
                Err(e) => {
                    error_msg = Some(format!("spawn_blocking failed: {e}"));
                    break 'read_loop;
                }
            };

            if n == 0 {
                tracing::trace!("execute_with_vt: PTY returned 0 bytes (closed)");
                error_msg = Some("PTY closed unexpectedly".to_string());
                break 'read_loop;
            }

            let chunk = String::from_utf8_lossy(&buf[..n]);
            vt.feed_str(&chunk);
            tracing::trace!("execute_with_vt: read {} bytes: {:?}", n, chunk);
            output_buffer.push_str(&chunk);

            while let Some(newline_pos) = output_buffer.find('\n') {
                let line = output_buffer[..newline_pos].to_string();
                output_buffer = output_buffer[newline_pos + 1..].to_string();

                let clean = strip_ansi_codes(&line);
                let trimmed = clean.trim();

                tracing::trace!("execute_with_vt: line (started={}): {:?}", started, trimmed);

                if !started && trimmed == start_marker {
                    tracing::trace!("execute_with_vt: found start marker");
                    started = true;
                    continue;
                }

                if started && trimmed.starts_with(&end_marker_prefix) && trimmed.ends_with("__") {
                    exit_code = trimmed
                        .strip_prefix(&end_marker_prefix)
                        .and_then(|s| s.strip_suffix("__"))
                        .and_then(|s| s.parse::<i32>().ok());
                    tracing::trace!(
                        "execute_with_vt: found end marker, exit_code={:?}",
                        exit_code
                    );
                    break 'read_loop;
                }

                if started {
                    stdout_lines.push((Instant::now(), line));
                }
            }
        }

        let (success, error) = if let Some(err) = error_msg {
            (false, Some(err))
        } else {
            let code = exit_code.unwrap_or(1);
            (
                code == 0,
                if code == 0 {
                    None
                } else {
                    Some(format!("Task exited with code {code}"))
                },
            )
        };

        tracing::trace!(
            "execute_with_vt: result success={}, error={:?}",
            success,
            error
        );

        let _ = request.response_tx.send(PtyTaskResult {
            success,
            stdout_lines,
            stderr_lines: Vec::new(),
            error,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_codes() {
        // Basic ANSI color code
        assert_eq!(strip_ansi_codes("\x1b[32mgreen\x1b[0m"), "green");

        // CSI sequence with multiple params
        assert_eq!(
            strip_ansi_codes("\x1b[1;32mbold green\x1b[0m"),
            "bold green"
        );

        // Bracketed paste mode
        assert_eq!(strip_ansi_codes("\x1b[?2004ltext"), "text");

        // No ANSI codes
        assert_eq!(strip_ansi_codes("plain text"), "plain text");

        // Mixed content
        assert_eq!(
            strip_ansi_codes("start\x1b[31mred\x1b[0mend"),
            "startredend"
        );
    }

    #[test]
    fn test_strip_ansi_codes_osc() {
        // OSC sequence (title change)
        assert_eq!(strip_ansi_codes("\x1b]0;title\x07text"), "text");
    }

    #[test]
    fn test_strip_ansi_codes_charset() {
        // Character set selection
        assert_eq!(strip_ansi_codes("\x1b(Btext"), "text");
    }
}
