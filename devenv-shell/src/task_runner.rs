//! Marker-based task execution in PTY.
//!
//! This module provides functionality to execute tasks inside a PTY shell
//! using a marker-based protocol to delimit command output.

use crate::protocol::{PtyTaskRequest, PtyTaskResult};
use crate::pty::Pty;
use std::sync::Arc;
use std::time::Instant;
use strip_ansi_escapes::strip_str;
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
            let escaped = cwd.replace('\'', "'\\''");
            cmd_parts.push(format!("cd '{escaped}'"));
        }

        // Execute the command.
        // Note: the command is expected to be a Nix store path (simple executable path),
        // not an arbitrary shell expression. The sudo path wraps it as a single argument
        // to `sudo -E`, so compound shell commands would not work correctly.
        if request.use_sudo {
            let escaped = request.command.replace('\'', "'\\''");
            cmd_parts.push(format!("sudo -E '{escaped}'"));
        } else {
            cmd_parts.push(request.command.clone());
        }

        // Echo end marker with exit code
        cmd_parts.push(format!("echo '{end_marker_prefix}'$?'__'"));

        // Join with semicolons and add newline.
        // History is disabled during task execution (set +o history in rcfile),
        // so these commands won't appear in user's shell history.
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
                let clean = strip_str(&line);
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

    /// Disable PROMPT_COMMAND to avoid ~100ms+ overhead per task from prompt hooks.
    /// History is already disabled from shell init, so we just save/clear PROMPT_COMMAND.
    fn disable_prompt_command(&self) -> Result<(), TaskRunnerError> {
        self.pty
            .write_all(b"__devenv_saved_pc=\"$PROMPT_COMMAND\"; PROMPT_COMMAND=\n")
            .map_err(|e| {
                TaskRunnerError::Pty(format!("Failed to disable PROMPT_COMMAND: {}", e))
            })?;
        self.pty
            .flush()
            .map_err(|e| TaskRunnerError::Pty(format!("Failed to flush PTY: {}", e)))
    }

    /// Restore PROMPT_COMMAND when handing control to user.
    /// History is re-enabled separately inside drain_pty_to_vt() so the
    /// sentinel echo commands are never recorded.
    fn restore_prompt_command(&self) -> Result<(), TaskRunnerError> {
        self.pty
            .write_all(b"PROMPT_COMMAND=\"$__devenv_saved_pc\"\n")
            .map_err(|e| {
                TaskRunnerError::Pty(format!("Failed to restore PROMPT_COMMAND: {}", e))
            })?;
        self.pty
            .flush()
            .map_err(|e| TaskRunnerError::Pty(format!("Failed to flush PTY: {}", e)))
    }

    /// Run task loop, processing requests until channel closes.
    pub async fn run_loop(
        &self,
        task_rx: &mut mpsc::Receiver<PtyTaskRequest>,
    ) -> Result<(), TaskRunnerError> {
        tracing::trace!("run_loop: waiting for task requests");
        self.disable_prompt_command()?;

        while let Some(request) = task_rx.recv().await {
            self.execute(request).await;
        }

        self.restore_prompt_command()?;
        // Re-enable history (no drain in this path, so send directly)
        self.pty
            .write_all(b" set -o history\n")
            .map_err(|e| TaskRunnerError::Pty(format!("Failed to re-enable history: {}", e)))?;
        self.pty
            .flush()
            .map_err(|e| TaskRunnerError::Pty(format!("Failed to flush PTY: {}", e)))?;
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
        self.disable_prompt_command()?;

        while let Some(request) = task_rx.recv().await {
            self.execute_with_vt(request, vt).await;
        }

        self.restore_prompt_command()?;

        // Drain any pending PTY output into VT so it doesn't leak to stdout later.
        // Uses a sentinel to deterministically consume all output without leaving
        // zombie threads that could steal future PTY reads.
        // History is still disabled at this point; the drain command itself
        // re-enables history after the sentinels so they are never recorded.
        self.drain_pty_to_vt(vt).await;

        tracing::trace!("run_with_vt: task channel closed, exiting");
        Ok(())
    }

    /// Drain pending PTY output into VT using a sentinel marker.
    ///
    /// Sends an echo command with a known sentinel string, then reads from the
    /// PTY until the sentinel appears. Each read is individually awaited (no
    /// infinite loop inside spawn_blocking), so no zombie threads are left behind
    /// that could steal future PTY reads from the session's reader thread.
    async fn drain_pty_to_vt(&self, vt: &mut avt::Vt) {
        let sentinel = "__DEVENV_DRAIN_DONE__";
        // Send the sentinel echo twice as separate command lines.
        // Reading until the second match ensures the PROMPT_COMMAND output
        // between them is consumed, leaving exactly one prompt in the buffer.
        // History is still disabled (set +o history from init), so the sentinel
        // commands are not recorded. We re-enable history after the second
        // sentinel so the drain consumes all internal command output.
        let cmd = format!(" echo '{sentinel}'\n echo '{sentinel}'; set -o history\n");
        if self.pty.write_all(cmd.as_bytes()).is_err() || self.pty.flush().is_err() {
            tracing::warn!("drain_pty_to_vt: failed to send sentinel command");
            return;
        }

        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        let mut buffer = String::new();
        let mut total_bytes = 0usize;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                tracing::warn!("drain_pty_to_vt: timed out waiting for sentinel");
                break;
            }

            let pty_clone = Arc::clone(&self.pty);
            let handle = tokio::task::spawn_blocking(move || {
                let mut buf = [0u8; 4096];
                pty_clone.read(&mut buf).map(|n| buf[..n].to_vec())
            });

            match tokio::time::timeout(remaining, handle).await {
                Ok(Ok(Ok(data))) if !data.is_empty() => {
                    let chunk = String::from_utf8_lossy(&data);
                    vt.feed_str(&chunk);
                    total_bytes += data.len();
                    buffer.push_str(&chunk);
                    // Check for sentinel on its own line (after stripping ANSI codes).
                    // This avoids matching the terminal echo of the command which
                    // contains the sentinel as part of: echo '__DEVENV_DRAIN_DONE__'
                    // We need TWO matches (one per echo command) to consume the
                    // PROMPT_COMMAND output between them.
                    let match_count = buffer
                        .lines()
                        .filter(|line| strip_str(line).trim() == sentinel)
                        .count();
                    if match_count >= 2 {
                        break;
                    }
                }
                _ => {
                    tracing::warn!("drain_pty_to_vt: read failed while waiting for sentinel");
                    break;
                }
            }
        }

        if total_bytes > 0 {
            tracing::trace!("drain_pty_to_vt: drained {} bytes", total_bytes);
        }
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
            let escaped = cwd.replace('\'', "'\\''");
            cmd_parts.push(format!("cd '{escaped}'"));
        }

        // Execute the command.
        // Note: the command is expected to be a Nix store path (simple executable path),
        // not an arbitrary shell expression. The sudo path wraps it as a single argument
        // to `sudo -E`, so compound shell commands would not work correctly.
        if request.use_sudo {
            let escaped = request.command.replace('\'', "'\\''");
            cmd_parts.push(format!("sudo -E '{escaped}'"));
        } else {
            cmd_parts.push(request.command.clone());
        }
        cmd_parts.push(format!("echo '{end_marker_prefix}'$?'__'"));

        // Prefix with space to prevent command from being saved to shell history
        let full_cmd = format!(" {}\n", cmd_parts.join("; "));

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

                let clean = strip_str(&line);
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
