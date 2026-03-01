use std::collections::BTreeMap;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Result of a task execution.
#[derive(Debug)]
pub struct ExecutionResult {
    pub success: bool,
    pub stdout_lines: Vec<(std::time::Instant, String)>,
    pub stderr_lines: Vec<(std::time::Instant, String)>,
    pub error: Option<String>,
}

/// Context for executing a task command.
pub struct ExecutionContext<'a> {
    /// The command to execute (path to script).
    pub command: &'a str,
    /// Working directory for the command.
    pub cwd: Option<&'a str>,
    /// Environment variables to set.
    pub env: BTreeMap<String, String>,
    /// Whether to run with sudo.
    pub use_sudo: bool,
    /// Path to the output file for DEVENV_TASK_OUTPUT_FILE.
    pub output_file_path: &'a std::path::Path,
}

/// Callback for streaming output lines during execution.
pub trait OutputCallback: Send + Sync {
    fn on_stdout(&self, line: &str);
    fn on_stderr(&self, line: &str);
}

/// A no-op output callback for when streaming is not needed.
pub struct NoOpCallback;

impl OutputCallback for NoOpCallback {
    fn on_stdout(&self, _line: &str) {}
    fn on_stderr(&self, _line: &str) {}
}

/// Executor that spawns task commands as subprocesses.
pub struct SubprocessExecutor;

impl SubprocessExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SubprocessExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl SubprocessExecutor {
    pub async fn execute(
        &self,
        ctx: ExecutionContext<'_>,
        callback: &dyn OutputCallback,
        cancellation: CancellationToken,
    ) -> ExecutionResult {
        use nix::sys::signal::{self as nix_signal, Signal};
        use nix::unistd::Pid;
        use std::process::Stdio;
        use tokio::io::{AsyncBufReadExt, BufReader};
        use tokio::process::Command;
        use tracing::error;

        // Build the command
        let mut command = if ctx.use_sudo {
            let mut sudo_cmd = Command::new("sudo");
            sudo_cmd.args(["-E", ctx.command]);
            sudo_cmd
        } else {
            Command::new(ctx.command)
        };

        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        // Set working directory if specified
        if let Some(cwd) = ctx.cwd {
            command.current_dir(cwd);
        }

        // Set environment variables
        for (key, value) in &ctx.env {
            command.env(key, value);
        }

        // Set DEVENV_TASK_OUTPUT_FILE
        command.env("DEVENV_TASK_OUTPUT_FILE", ctx.output_file_path);

        // Spawn the process
        let mut child = match command.spawn() {
            Ok(c) => c,
            Err(e) => {
                return ExecutionResult {
                    success: false,
                    stdout_lines: Vec::new(),
                    stderr_lines: Vec::new(),
                    error: Some(format!("Failed to spawn command for {}: {e}", ctx.command)),
                };
            }
        };

        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                return ExecutionResult {
                    success: false,
                    stdout_lines: Vec::new(),
                    stderr_lines: Vec::new(),
                    error: Some("Failed to capture stdout".to_string()),
                };
            }
        };

        let stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => {
                return ExecutionResult {
                    success: false,
                    stdout_lines: Vec::new(),
                    stderr_lines: Vec::new(),
                    error: Some("Failed to capture stderr".to_string()),
                };
            }
        };

        let mut stderr_reader = BufReader::new(stderr).lines();
        let mut stdout_reader = BufReader::new(stdout).lines();

        let mut stdout_lines = Vec::new();
        let mut stderr_lines = Vec::new();

        let mut stdout_closed = false;
        let mut stderr_closed = false;
        let mut exit_status: Option<std::process::ExitStatus> = None;

        loop {
            if exit_status.is_some() && stdout_closed && stderr_closed {
                break;
            }

            tokio::select! {
                result = stdout_reader.next_line(), if !stdout_closed => {
                    match result {
                        Ok(Some(line)) => {
                            callback.on_stdout(&line);
                            stdout_lines.push((std::time::Instant::now(), line));
                        },
                        Ok(None) => {
                            stdout_closed = true;
                        },
                        Err(e) => {
                            error!("Error reading stdout: {}", e);
                            stderr_lines.push((std::time::Instant::now(), e.to_string()));
                            stdout_closed = true;
                        },
                    }
                }
                result = stderr_reader.next_line(), if !stderr_closed => {
                    match result {
                        Ok(Some(line)) => {
                            callback.on_stderr(&line);
                            stderr_lines.push((std::time::Instant::now(), line));
                        },
                        Ok(None) => {
                            stderr_closed = true;
                        },
                        Err(e) => {
                            error!("Error reading stderr: {}", e);
                            stderr_lines.push((std::time::Instant::now(), e.to_string()));
                            stderr_closed = true;
                        },
                    }
                }
                result = child.wait(), if exit_status.is_none() => {
                    match result {
                        Ok(status) => {
                            exit_status = Some(status);
                        },
                        Err(e) => {
                            error!("Error waiting for command: {}", e);
                            return ExecutionResult {
                                success: false,
                                stdout_lines,
                                stderr_lines,
                                error: Some(format!("Error waiting for command: {e}")),
                            };
                        }
                    }
                }
                _ = cancellation.cancelled() => {
                    // Kill the child process and its process group
                    if let Some(pid) = child.id() {
                        // Send SIGTERM to the process group first for graceful shutdown
                        let _ = nix_signal::killpg(Pid::from_raw(pid as i32), Signal::SIGTERM);

                        tokio::select! {
                            _ = child.wait() => {
                                // Process exited gracefully
                            }
                            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                                // Grace period expired, send SIGKILL
                                let _ = nix_signal::killpg(Pid::from_raw(pid as i32), Signal::SIGKILL);
                                let _ = child.wait().await;
                            }
                        }
                    }

                    return ExecutionResult {
                        success: false,
                        stdout_lines,
                        stderr_lines,
                        error: Some("Task cancelled".to_string()),
                    };
                }
            }
        }

        let success = exit_status.map(|s| s.success()).unwrap_or(false);
        ExecutionResult {
            success,
            stdout_lines,
            stderr_lines,
            error: if success {
                None
            } else {
                Some(format!(
                    "Task exited with status: {}",
                    exit_status
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                ))
            },
        }
    }
}
