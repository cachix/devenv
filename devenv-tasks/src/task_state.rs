use crate::SudoContext;
use crate::config::TaskConfig;
use crate::task_cache::{TaskCache, expand_glob_patterns};
use crate::types::{Output, Skipped, TaskCompleted, TaskFailure, TaskStatus, VerbosityLevel};
use devenv_activity::{Activity, ActivityLevel};
use miette::{IntoDiagnostic, Result, WrapErr};
use nix::sys::signal::{self as nix_signal, Signal};
use nix::unistd::Pid;
use std::collections::BTreeMap;
use std::process::Stdio;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::error;

impl std::fmt::Debug for TaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskState")
            .field("task", &self.task)
            .field("status", &self.status)
            .field("verbosity", &self.verbosity)
            .finish()
    }
}

pub struct TaskState {
    pub task: TaskConfig,
    pub status: TaskStatus,
    pub verbosity: VerbosityLevel,
    pub sudo_context: Option<SudoContext>,
}

impl TaskState {
    pub fn new(
        task: TaskConfig,
        verbosity: VerbosityLevel,
        sudo_context: Option<SudoContext>,
    ) -> Self {
        Self {
            task,
            status: TaskStatus::Pending,
            verbosity,
            sudo_context,
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

        // Include the command path in the files to check, so that
        // changes to the task's exec script or dependencies will invalidate the cache.
        // This works because Nix store paths are content-addressed.
        let mut files_to_check = self.task.exec_if_modified.clone();
        if let Some(cmd) = &self.task.command {
            files_to_check.push(cmd.clone());
        }

        cache
            .check_modified_files(&self.task.name, &files_to_check)
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
        // If we dropped privileges but have sudo context, restore sudo for the task
        let mut command = if let Some(_ctx) = &self.sudo_context {
            // Wrap with sudo to restore elevated privileges
            let mut sudo_cmd = Command::new("sudo");
            // Use -E to preserve environment variables
            // The command here is a store path to a task script, not an arbitrary shell command.
            sudo_cmd.args(["-E", cmd]);
            sudo_cmd
        } else {
            // Normal execution - no sudo involved
            Command::new(cmd)
        };

        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        // Set working directory if specified
        if let Some(cwd) = &self.task.cwd {
            command.current_dir(cwd);
        }

        // Set DEVENV_TASK_INPUT
        if let Some(input) = &self.task.input {
            let input_json = serde_json::to_string(input)
                .into_diagnostic()
                .wrap_err("Failed to serialize task input to JSON")?;
            command.env("DEVENV_TASK_INPUT", input_json);
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
            if let Some(env) = value.get("devenv").and_then(|d| d.get("env"))
                && let Some(env_obj) = env.as_object()
            {
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

    pub async fn run(
        &self,
        now: Instant,
        outputs: &BTreeMap<String, serde_json::Value>,
        cache: &TaskCache,
        cancellation: CancellationToken,
    ) -> Result<TaskCompleted> {
        // Create a Task activity for tracking this task's lifecycle.
        // All child activities created within scope() will have this as their parent.
        let task_activity = Activity::task(&self.task.name)
            .show_output(self.task.show_output)
            .is_process(self.task.r#type == crate::types::TaskType::Process)
            .start();

        // Run the entire task within the activity's scope for proper parent-child nesting
        task_activity
            .scope(self.run_inner(now, outputs, cache, cancellation, &task_activity))
            .await
    }

    async fn run_inner(
        &self,
        now: Instant,
        outputs: &BTreeMap<String, serde_json::Value>,
        cache: &TaskCache,
        cancellation: CancellationToken,
        task_activity: &Activity,
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

            // Create a Command activity for the status check (automatically parented to task_activity)
            let status_activity = Activity::command(&self.task.name)
                .command(cmd)
                .level(ActivityLevel::Debug)
                .start();

            match command.output().await {
                Ok(output) => {
                    if !output.status.success() {
                        status_activity.fail();
                    }

                    if output.status.success() {
                        let output = Output(cached_output);
                        tracing::debug!("Task {} skipped with output: {:?}", task_name, output);
                        task_activity.cached();
                        return Ok(TaskCompleted::Skipped(Skipped::Cached(output)));
                    }
                }
                Err(e) => {
                    status_activity.fail();
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
                task_activity.cached();
                return Ok(TaskCompleted::Skipped(Skipped::Cached(Output(task_output))));
            }
        }

        let Some(cmd) = &self.task.command else {
            task_activity.skipped();
            return Ok(TaskCompleted::Skipped(Skipped::NoCommand));
        };

        // Create a Command activity for the main execution (automatically parented to task_activity)
        let cmd_activity = Activity::command(&self.task.name)
            .command(cmd)
            .level(ActivityLevel::Debug)
            .start();

        let (mut command, outputs_file) = self
            .prepare_command(cmd, outputs)
            .wrap_err("Failed to prepare task command")?;

        let result = command
            .spawn()
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to spawn command for {cmd}"));

        let mut child = match result {
            Ok(c) => c,
            Err(err) => {
                cmd_activity.fail();
                task_activity.fail();
                return Ok(TaskCompleted::Failed(
                    now.elapsed(),
                    TaskFailure {
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                        error: format!("{err:#}"),
                    },
                ));
            }
        };

        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                cmd_activity.fail();
                task_activity.fail();
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
                cmd_activity.fail();
                task_activity.fail();
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
        let mut exit_status: Option<std::process::ExitStatus> = None;

        loop {
            // If child has exited and both pipes are closed, we're done
            if exit_status.is_some() && stdout_closed && stderr_closed {
                break;
            }

            tokio::select! {
                result = stdout_reader.next_line(), if !stdout_closed => {
                    match result {
                        Ok(Some(line)) => {
                            task_activity.log(&line);
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
                            task_activity.error(&line);
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
                            if !status.success() {
                                cmd_activity.fail();
                            }

                            // Update the file states to capture any changes the task made,
                            // regardless of whether the task succeeded or failed
                            let expanded_paths = expand_glob_patterns(&self.task.exec_if_modified);
                            for path in expanded_paths {
                                cache.update_file_state(&self.task.name, &path).await?;
                            }

                            exit_status = Some(status);
                        },
                        Err(e) => {
                            error!("{}> Error waiting for command: {}", self.task.name, e);
                            cmd_activity.fail();
                            task_activity.fail();
                            return Ok(TaskCompleted::Failed(
                                now.elapsed(),
                                TaskFailure {
                                    stdout: stdout_lines,
                                    stderr: stderr_lines,
                                    error: format!("Error waiting for command: {e}"),
                                },
                            ));
                        }
                    }
                }
                _ = cancellation.cancelled() => {
                    eprintln!("Task {} received shutdown signal, terminating child process", self.task.name);

                    // Kill the child process and its process group
                    if let Some(pid) = child.id() {
                        // Send SIGTERM to the process group first for graceful shutdown
                        let _ = nix_signal::killpg(Pid::from_raw(pid as i32), Signal::SIGTERM);

                        tokio::select! {
                            _ = child.wait() => {
                                // Process exited gracefully
                            }
                            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                                // Grace period expired, send SIGKILL
                                let _ = nix_signal::killpg(Pid::from_raw(pid as i32), Signal::SIGKILL);
                                let _ = child.wait().await;
                            }
                        }
                    }

                    cmd_activity.cancel();
                    task_activity.cancel();
                    return Ok(TaskCompleted::Cancelled(Some(now.elapsed())));
                }
            }
        }

        let status = exit_status.expect("Loop exited without exit status");
        if status.success() {
            Ok(TaskCompleted::Success(
                now.elapsed(),
                Self::get_outputs(&outputs_file).await,
            ))
        } else {
            task_activity.fail();
            Ok(TaskCompleted::Failed(
                now.elapsed(),
                TaskFailure {
                    stdout: stdout_lines,
                    stderr: stderr_lines,
                    error: format!("Task exited with status: {status}"),
                },
            ))
        }
    }
}
