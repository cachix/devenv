use crate::config::TaskConfig;
use crate::task_cache::{expand_glob_patterns, TaskCache};
use crate::types::{Output, Skipped, TaskCompleted, TaskFailure, TaskStatus, VerbosityLevel};
use miette::{IntoDiagnostic, Result, WrapErr};
use std::collections::BTreeMap;
use std::process::Stdio;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{error, instrument};

#[derive(Debug)]
pub struct TaskState {
    pub task: TaskConfig,
    pub status: TaskStatus,
    pub verbosity: VerbosityLevel,
    pub cancellation_token: Option<CancellationToken>,
}

impl TaskState {
    pub fn new(
        task: TaskConfig,
        verbosity: VerbosityLevel,
        cancellation_token: Option<CancellationToken>,
    ) -> Self {
        Self {
            task,
            status: TaskStatus::Pending,
            verbosity,
            cancellation_token,
        }
    }

    /// Handle file modification checking with centralized error handling.
    /// Returns a Result with a boolean indicating if files were modified.
    async fn check_files_modified_result(
        &self,
        cache: &TaskCache,
    ) -> Result<bool, devenv_cache_core::error::CacheError> {
        if self.task.exec_if_modified.is_empty() {
            return Ok(false);
        }

        cache
            .check_modified_files(&self.task.name, &self.task.exec_if_modified)
            .await
    }

    /// Check if any files specified in exec_if_modified have been modified.
    /// Returns true if any files have been modified or if there was an error checking.
    async fn check_modified_files(&self, cache: &TaskCache) -> bool {
        match self.check_files_modified_result(cache).await {
            Ok(modified) => modified,
            Err(e) => {
                // Log the error and default to running the task if there's an error
                tracing::warn!(
                    "Failed to check modified files for task {}: {}",
                    self.task.name,
                    e
                );
                true
            }
        }
    }

    fn prepare_command(
        &self,
        cmd: &str,
        outputs: &BTreeMap<String, serde_json::Value>,
    ) -> Result<(Command, tempfile::NamedTempFile)> {
        let mut command = Command::new(cmd);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        // Set working directory if specified
        if let Some(cwd) = &self.task.cwd {
            command.current_dir(cwd);
        }

        // Create a new process group for better signal handling
        // This ensures that signals sent to the parent are propagated to all children
        command.process_group(0);

        // Set DEVENV_TASK_INPUTS
        if let Some(inputs) = &self.task.inputs {
            let inputs_json = serde_json::to_string(inputs)
                .into_diagnostic()
                .wrap_err("Failed to serialize task inputs to JSON")?;
            command.env("DEVENV_TASK_INPUT", inputs_json);
        }

        // Create a temporary file for DEVENV_TASK_OUTPUT_FILE
        let outputs_file = tempfile::Builder::new()
            .prefix("devenv_task_output")
            .suffix(".json")
            .tempfile()
            .into_diagnostic()
            .wrap_err("Failed to create temporary file for task output")?;
        command.env("DEVENV_TASK_OUTPUT_FILE", outputs_file.path());

        // Set environment variables from task outputs
        let mut devenv_env = String::new();
        for (_, value) in outputs.iter() {
            if let Some(env) = value.get("devenv").and_then(|d| d.get("env")) {
                if let Some(env_obj) = env.as_object() {
                    for (env_key, env_value) in env_obj {
                        if let Some(env_str) = env_value.as_str() {
                            command.env(env_key, env_str);
                            devenv_env.push_str(&format!(
                                "export {}={}\n",
                                env_key,
                                shell_escape::escape(std::borrow::Cow::Borrowed(env_str))
                            ));
                        }
                    }
                }
            }
        }
        // Internal for now
        command.env("DEVENV_TASK_ENV", devenv_env);

        // Set DEVENV_TASKS_OUTPUTS
        let outputs_json = serde_json::to_string(outputs)
            .into_diagnostic()
            .wrap_err("Failed to serialize task outputs to JSON")?;
        command.env("DEVENV_TASKS_OUTPUTS", outputs_json);

        Ok((command, outputs_file))
    }

    async fn get_outputs(outputs_file: &tempfile::NamedTempFile) -> Output {
        let output = match File::open(outputs_file.path()).await {
            Ok(mut file) => {
                let mut contents = String::new();
                // TODO: report JSON parsing errors
                file.read_to_string(&mut contents).await.ok();
                serde_json::from_str(&contents).ok()
            }
            Err(_) => None,
        };
        Output(output)
    }

    #[instrument(ret)]
    pub async fn run(
        &self,
        now: Instant,
        outputs: &BTreeMap<String, serde_json::Value>,
        cache: &TaskCache,
    ) -> Result<TaskCompleted> {
        tracing::debug!(
            "Running task '{}' with exec_if_modified: {:?}, status: {}",
            self.task.name,
            self.task.exec_if_modified,
            self.task.status.is_some()
        );

        // Check if we should run based on the status command
        if let Some(cmd) = &self.task.status {
            // First check if we have cached output from a previous run
            let task_name = &self.task.name;
            let cached_output = match cache.get_task_output(task_name).await {
                Ok(Some(output)) => {
                    tracing::debug!("Found cached output for task {} in database", task_name);
                    Some(output)
                }
                Ok(None) => {
                    tracing::debug!("No cached output found for task {}", task_name);
                    None
                }
                Err(e) => {
                    tracing::warn!("Failed to get cached output for task {}: {}", task_name, e);
                    None
                }
            };

            let (mut command, _) = self
                .prepare_command(cmd, outputs)
                .wrap_err("Failed to prepare status command")?;

            // Use spawn and wait with output to properly handle status script execution
            match command.output().await {
                Ok(output) => {
                    if output.status.success() {
                        let output = Output(cached_output);
                        tracing::debug!("Task {} skipped with output: {:?}", task_name, output);
                        return Ok(TaskCompleted::Skipped(Skipped::Cached(output)));
                    }
                }
                Err(e) => {
                    // TODO: stdout, stderr
                    return Ok(TaskCompleted::Failed(
                        now.elapsed(),
                        TaskFailure {
                            stdout: Vec::new(),
                            stderr: Vec::new(),
                            error: e.to_string(),
                        },
                    ));
                }
            }
        } else if !self.task.exec_if_modified.is_empty() {
            tracing::debug!(
                "Task '{}' has exec_if_modified files: {:?}",
                self.task.name,
                self.task.exec_if_modified
            );

            let files_modified = self.check_modified_files(cache).await;
            tracing::debug!(
                "Task '{}' files modified check result: {}",
                self.task.name,
                files_modified
            );

            if !files_modified {
                // If no status command but we have paths to check, and none are modified,
                // First check if we have outputs in the current run's outputs map
                let mut task_output = outputs.get(&self.task.name).cloned();

                // If not, try to load from the cache
                if task_output.is_none() {
                    match cache.get_task_output(&self.task.name).await {
                        Ok(Some(cached_output)) => {
                            tracing::debug!(
                                "Found cached output for task {} in database",
                                self.task.name
                            );
                            task_output = Some(cached_output);
                        }
                        Ok(None) => {
                            tracing::debug!("No cached output found for task {}", self.task.name);
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to get cached output for task {}: {}",
                                self.task.name,
                                e
                            );
                        }
                    }
                }

                tracing::debug!(
                    "Skipping task {} due to unmodified files, output: {:?}",
                    self.task.name,
                    task_output
                );
                return Ok(TaskCompleted::Skipped(Skipped::Cached(Output(task_output))));
            }
        }
        if let Some(cmd) = &self.task.command {
            let (mut command, outputs_file) = self
                .prepare_command(cmd, outputs)
                .wrap_err("Failed to prepare task command")?;

            let result = command
                .spawn()
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to spawn command for {}", cmd));

            let mut child = match result {
                Ok(c) => c,
                Err(err) => {
                    return Ok(TaskCompleted::Failed(
                        now.elapsed(),
                        TaskFailure {
                            stdout: Vec::new(),
                            stderr: Vec::new(),
                            error: format!("{:#}", err),
                        },
                    ));
                }
            };

            let stdout = match child.stdout.take() {
                Some(stdout) => stdout,
                None => {
                    return Ok(TaskCompleted::Failed(
                        now.elapsed(),
                        TaskFailure {
                            stdout: Vec::new(),
                            stderr: Vec::new(),
                            error: "Failed to capture stdout".to_string(),
                        },
                    ));
                }
            };
            let stderr = match child.stderr.take() {
                Some(stderr) => stderr,
                None => {
                    return Ok(TaskCompleted::Failed(
                        now.elapsed(),
                        TaskFailure {
                            stdout: Vec::new(),
                            stderr: Vec::new(),
                            error: "Failed to capture stderr".to_string(),
                        },
                    ));
                }
            };

            let mut stderr_reader = BufReader::new(stderr).lines();
            let mut stdout_reader = BufReader::new(stdout).lines();

            let mut stdout_lines = Vec::new();
            let mut stderr_lines = Vec::new();

            // Track EOF status for stdout and stderr streams
            let mut stdout_closed = false;
            let mut stderr_closed = false;

            // Check if this is a process task (always show output for processes)
            let is_process = self.task.name.starts_with("devenv:processes:");

            loop {
                tokio::select! {
                    // Check for cancellation from shared signal handler
                    _ = async {
                        if let Some(ref token) = self.cancellation_token {
                            token.cancelled().await
                        } else {
                            std::future::pending::<()>().await
                        }
                    } => {
                        eprintln!("Task {} received shutdown signal, terminating child process", self.task.name);

                        // Kill the child process and its process group
                        if let Some(pid) = child.id() {
                            use ::nix::sys::signal::{self, Signal};
                            use ::nix::unistd::Pid;

                            // Send SIGTERM to the process group first for graceful shutdown
                            signal::killpg(Pid::from_raw(pid as i32), Signal::SIGTERM).expect("failed to send SIGTERM to process group");
                            tokio::select! {
                                _ = child.wait() => {}
                                _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                                        signal::killpg(Pid::from_raw(pid as i32), Signal::SIGKILL).expect("failed to send SIGKILL to process group");
                                        child.wait().await.expect("failed to wait on child process");
                                }
                            }
                        }

                        return Ok(TaskCompleted::Cancelled(now.elapsed()));
                    }
                    result = stdout_reader.next_line(), if !stdout_closed => {
                        match result {
                            Ok(Some(line)) => {
                                if self.verbosity == VerbosityLevel::Verbose || is_process {
                                    eprintln!("[{}] {}", self.task.name, line);
                                }
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
                                if self.verbosity == VerbosityLevel::Verbose || is_process {
                                    eprintln!("[{}] {}", self.task.name, line);
                                }
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
                    result = child.wait() => {
                        match result {
                            Ok(status) => {
                                // Update the file states to capture any changes the task made,
                                // regardless of whether the task succeeded or failed
                                let expanded_paths = expand_glob_patterns(&self.task.exec_if_modified);
                                for path in expanded_paths {
                                    cache.update_file_state(&self.task.name, &path).await?;
                                }

                                if status.success() {
                                    return Ok(TaskCompleted::Success(now.elapsed(), Self::get_outputs(&outputs_file).await));
                                } else {
                                    return Ok(TaskCompleted::Failed(
                                        now.elapsed(),
                                        TaskFailure {
                                            stdout: stdout_lines,
                                            stderr: stderr_lines,
                                            error: format!("Task exited with status: {}", status),
                                        },
                                    ));
                                }
                            },
                            Err(e) => {
                                error!("{}> Error waiting for command: {}", self.task.name, e);
                                return Ok(TaskCompleted::Failed(
                                    now.elapsed(),
                                    TaskFailure {
                                        stdout: stdout_lines,
                                        stderr: stderr_lines,
                                        error: format!("Error waiting for command: {}", e),
                                    },
                                ));
                            }
                        }
                    }
                }
            }
        } else {
            return Ok(TaskCompleted::Skipped(Skipped::NoCommand));
        }
    }
}
