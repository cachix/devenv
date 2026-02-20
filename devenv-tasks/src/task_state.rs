use crate::SudoContext;
use crate::config::TaskConfig;
use crate::executor::{ExecutionContext, OutputCallback, TaskExecutor};
use crate::task_cache::{TaskCache, expand_glob_patterns};
use crate::types::{Output, Skipped, TaskCompleted, TaskFailure, TaskStatus, VerbosityLevel};
use devenv_activity::{Activity, ActivityInstrument, ActivityLevel};
use devenv_processes::{ListenKind, NativeProcessManager, ProcessConfig};
use miette::{IntoDiagnostic, Result, WrapErr};
use std::collections::BTreeMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

impl std::fmt::Debug for TaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskState")
            .field("task", &self.task)
            .field("status", &self.status)
            .field("verbosity", &self.verbosity)
            .finish()
    }
}

/// OutputCallback implementation that forwards output to an Activity.
struct ActivityCallback<'a> {
    activity: &'a Activity,
}

impl<'a> ActivityCallback<'a> {
    fn new(activity: &'a Activity) -> Self {
        Self { activity }
    }
}

impl OutputCallback for ActivityCallback<'_> {
    fn on_stdout(&self, line: &str) {
        self.activity.log(line);
    }

    fn on_stderr(&self, line: &str) {
        self.activity.error(line);
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

    /// Prepare environment variables for task execution.
    /// Returns the environment map and a tempfile for task output.
    fn prepare_env(
        &self,
        outputs: &BTreeMap<String, serde_json::Value>,
        shell_env: &std::collections::HashMap<String, String>,
    ) -> Result<(BTreeMap<String, String>, tempfile::NamedTempFile)> {
        // Start with shell env as the base layer
        let mut env: BTreeMap<String, String> = shell_env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        // Set DEVENV_TASK_INPUT
        if let Some(input) = &self.task.input {
            let input_json = serde_json::to_string(input)
                .into_diagnostic()
                .wrap_err("Failed to serialize task input to JSON")?;
            env.insert("DEVENV_TASK_INPUT".to_string(), input_json);
        }

        // Create a temporary file for DEVENV_TASK_OUTPUT_FILE
        let outputs_file = tempfile::Builder::new()
            .prefix("devenv_task_output")
            .suffix(".json")
            .tempfile()
            .into_diagnostic()
            .wrap_err("Failed to create temporary file for task output")?;

        // Set environment variables from task outputs
        let mut devenv_env = String::new();
        for (_, value) in outputs.iter() {
            if let Some(env_obj) = value
                .get("devenv")
                .and_then(|d| d.get("env"))
                .and_then(|e| e.as_object())
            {
                for (env_key, env_value) in env_obj {
                    if let Some(env_str) = env_value.as_str() {
                        env.insert(env_key.clone(), env_str.to_string());
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
        env.insert("DEVENV_TASK_ENV".to_string(), devenv_env);

        // Merge per-task env vars (take precedence over upstream exports)
        for (key, value) in &self.task.env {
            env.insert(key.clone(), value.clone());
        }

        // Set DEVENV_TASKS_OUTPUTS
        let outputs_json = serde_json::to_string(outputs)
            .into_diagnostic()
            .wrap_err("Failed to serialize task outputs to JSON")?;
        env.insert("DEVENV_TASKS_OUTPUTS".to_string(), outputs_json);

        Ok((env, outputs_file))
    }

    fn prepare_command(
        &self,
        cmd: &str,
        outputs: &BTreeMap<String, serde_json::Value>,
        shell_env: &std::collections::HashMap<String, String>,
    ) -> Result<(Command, tempfile::NamedTempFile)> {
        let (env, outputs_file) = self.prepare_env(outputs, shell_env)?;

        // Wrap with sudo if the task requires it (per-task use_sudo or global sudo context)
        let mut command = if self.task.use_sudo || self.sudo_context.is_some() {
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
            let cwd_path = std::path::Path::new(cwd);
            if !cwd_path.exists() {
                miette::bail!(
                    "Working directory for task '{}' does not exist: {}",
                    self.task.name,
                    cwd
                );
            }
            if !cwd_path.is_dir() {
                miette::bail!(
                    "Working directory for task '{}' is not a directory: {}",
                    self.task.name,
                    cwd
                );
            }
            command.current_dir(cwd);
        }

        // Set DEVENV_TASK_INPUT
        if let Some(input) = &self.task.input {
            let input_json = serde_json::to_string(input)
                .into_diagnostic()
                .wrap_err("Failed to serialize task input to JSON")?;
            command.env("DEVENV_TASK_INPUT", input_json);
        }

        // Set environment variables
        for (key, value) in &env {
            command.env(key, value);
        }

        // Set DEVENV_TASK_OUTPUT_FILE
        command.env("DEVENV_TASK_OUTPUT_FILE", outputs_file.path());

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

    /// Run a process task (long-running)
    ///
    /// This spawns a process using NativeProcessManager and immediately returns
    /// ProcessReady status. The process will stay alive until explicitly stopped
    /// via the process manager's stop_all().
    pub async fn run_process(
        &mut self,
        manager: &Arc<NativeProcessManager>,
        parent_id: Option<u64>,
        env: &std::collections::HashMap<String, String>,
    ) -> Result<()> {
        let Some(cmd) = &self.task.command else {
            return Err(miette::miette!(
                "Process task {} has no command",
                self.task.name
            ));
        };

        tracing::info!("Starting process task: {}", self.task.name);

        // Use short process name (strip "devenv:processes:" prefix for display)
        let process_name = self
            .task
            .name
            .strip_prefix("devenv:processes:")
            .unwrap_or(&self.task.name)
            .to_string();

        // Build process config, merging task config with process-specific overrides
        let mut config = if let Some(ref process) = self.task.process {
            ProcessConfig {
                name: process_name,
                exec: cmd.clone(),
                cwd: self.task.cwd.clone().map(std::path::PathBuf::from),
                ..process.clone()
            }
        } else {
            ProcessConfig {
                name: process_name,
                exec: cmd.clone(),
                cwd: self.task.cwd.clone().map(std::path::PathBuf::from),
                ..Default::default()
            }
        };

        // Propagate task-level use_sudo to process config
        config.use_sudo = self.task.use_sudo;

        // Merge devenv shell environment into process config
        // Task-level env takes precedence over shell env,
        // process-specific env takes precedence over both
        let mut merged_env = env.clone();
        merged_env.extend(self.task.env.clone());
        merged_env.extend(config.env.clone());
        config.env = merged_env;

        // Check if we need to wait for readiness (ready config, has listen sockets, or has allocated ports)
        let requires_ready_wait = config.ready.is_some()
            || config
                .listen
                .iter()
                .any(|spec| spec.kind == ListenKind::Tcp)
            || !config.ports.is_empty();

        // Start the process via the manager (which tracks it for shutdown)
        manager.start_command(&config, parent_id).await?;

        // Wait for ready signal if notify is enabled or has listen sockets
        if requires_ready_wait {
            tracing::info!("Waiting for process {} to signal ready...", self.task.name);
            manager.wait_ready(&config.name).await?;
            tracing::info!("Process {} signaled ready", self.task.name);
        }

        // Transition to ProcessReady
        self.status = TaskStatus::ProcessReady;

        tracing::info!("Process task {} is ready", self.task.name);

        Ok(())
    }

    /// Process DEVENV_EXPORT lines from task stdout and merge into output file.
    ///
    /// Format: DEVENV_EXPORT:<base64-var>=<base64-value>
    /// This allows tasks to export env vars without needing the devenv-tasks binary.
    async fn process_exports(
        stdout_lines: &[(std::time::Instant, String)],
        outputs_file: &tempfile::NamedTempFile,
    ) {
        use base64::Engine;
        use std::io::Write;

        let mut exports: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();

        for (_, line) in stdout_lines {
            if let Some(rest) = line.strip_prefix("DEVENV_EXPORT:") {
                // Format: <base64-key>=<base64-value>
                // Base64 uses '=' for padding, so split_once('=') would split
                // inside the key's padding. Instead, find the separator '=' at the
                // first position that's a multiple of 4 (end of a valid base64 string).
                let split_pos = (4..rest.len())
                    .step_by(4)
                    .find(|&i| rest.as_bytes()[i] == b'=');
                if let Some(pos) = split_pos {
                    let var_b64 = &rest[..pos];
                    let val_b64 = &rest[pos + 1..];
                    let engine = base64::engine::general_purpose::STANDARD;
                    if let (Ok(var_bytes), Ok(val_bytes)) =
                        (engine.decode(var_b64), engine.decode(val_b64))
                    {
                        if let (Ok(var), Ok(val)) =
                            (String::from_utf8(var_bytes), String::from_utf8(val_bytes))
                        {
                            exports.insert(var, serde_json::Value::String(val));
                        }
                    }
                }
            }
        }

        if exports.is_empty() {
            return;
        }

        // Read existing output file content
        let mut output: serde_json::Value = std::fs::read_to_string(outputs_file.path())
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_else(|| serde_json::json!({}));

        // Ensure devenv.env structure exists
        if !output.get("devenv").is_some() {
            output["devenv"] = serde_json::json!({});
        }
        if !output["devenv"].get("env").is_some() {
            output["devenv"]["env"] = serde_json::json!({});
        }

        // Merge exports into devenv.env
        if let Some(env_obj) = output["devenv"]["env"].as_object_mut() {
            for (k, v) in exports {
                env_obj.insert(k, v);
            }
        }

        // Write back to file
        if let Ok(content) = serde_json::to_string_pretty(&output) {
            let _ = std::fs::File::create(outputs_file.path())
                .and_then(|mut f| f.write_all(content.as_bytes()));
        }
    }

    /// Run this task with a pre-assigned activity ID.
    /// The Task::Hierarchy event has already been emitted; this emits Task::Start.
    pub async fn run(
        &self,
        now: Instant,
        outputs: &BTreeMap<String, serde_json::Value>,
        cache: &TaskCache,
        cancellation: CancellationToken,
        activity_id: u64,
        executor: &dyn TaskExecutor,
        refresh_task_cache: bool,
        shell_env: &std::collections::HashMap<String, String>,
    ) -> Result<TaskCompleted> {
        // Create the Activity with the pre-assigned ID - this emits Task::Start
        let task_activity = Activity::task_with_id(activity_id);

        // Run the entire task within the activity's scope for proper parent-child nesting
        self.run_inner(
            now,
            outputs,
            cache,
            cancellation,
            &task_activity,
            executor,
            refresh_task_cache,
            shell_env,
        )
        .in_activity(&task_activity)
        .await
    }

    async fn run_inner(
        &self,
        now: Instant,
        outputs: &BTreeMap<String, serde_json::Value>,
        cache: &TaskCache,
        cancellation: CancellationToken,
        task_activity: &Activity,
        executor: &dyn TaskExecutor,
        refresh_task_cache: bool,
        shell_env: &std::collections::HashMap<String, String>,
    ) -> Result<TaskCompleted> {
        tracing::debug!(
            "Running task '{}' with exec_if_modified: {:?}, status: {}",
            self.task.name,
            self.task.exec_if_modified,
            self.task.status.is_some()
        );

        // Check if we should skip based on cache (status command or exec_if_modified)
        if !refresh_task_cache {
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
                    .prepare_command(cmd, outputs, shell_env)
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
                                tracing::debug!(
                                    "No cached output found for task {}",
                                    self.task.name
                                );
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

        // Validate working directory if specified
        if let Some(cwd) = &self.task.cwd {
            let cwd_path = std::path::Path::new(cwd);
            if !cwd_path.exists() {
                miette::bail!(
                    "Working directory for task '{}' does not exist: {}",
                    self.task.name,
                    cwd
                );
            }
            if !cwd_path.is_dir() {
                miette::bail!(
                    "Working directory for task '{}' is not a directory: {}",
                    self.task.name,
                    cwd
                );
            }
        }

        // Prepare environment and output file
        let (env, outputs_file) = self
            .prepare_env(outputs, shell_env)
            .wrap_err("Failed to prepare task environment")?;

        // Build execution context
        let ctx = ExecutionContext {
            command: cmd,
            cwd: self.task.cwd.as_deref(),
            env,
            use_sudo: self.task.use_sudo || self.sudo_context.is_some(),
            output_file_path: outputs_file.path(),
        };

        // Execute using the provided executor
        let callback = ActivityCallback::new(task_activity);
        let result = executor.execute(ctx, &callback, cancellation).await;

        // Process any DEVENV_EXPORT lines from stdout and merge into output file
        Self::process_exports(&result.stdout_lines, &outputs_file).await;

        // Only update file states on success - failed tasks should not be cached
        if result.success {
            // Include command path in the files to update, matching check_files_modified
            let mut files_to_update = self.task.exec_if_modified.clone();
            if let Some(cmd) = &self.task.command {
                files_to_update.push(cmd.clone());
            }
            let expanded_paths = expand_glob_patterns(&files_to_update);
            for path in expanded_paths {
                cache.update_file_state(&self.task.name, &path).await?;
            }
        }

        if result.error.as_deref() == Some("Task cancelled") {
            cmd_activity.cancel();
            task_activity.cancel();
            return Ok(TaskCompleted::Cancelled(Some(now.elapsed())));
        }

        if result.success {
            Ok(TaskCompleted::Success(
                now.elapsed(),
                Self::get_outputs(&outputs_file).await,
            ))
        } else {
            cmd_activity.fail();
            task_activity.fail();
            Ok(TaskCompleted::Failed(
                now.elapsed(),
                TaskFailure {
                    stdout: result.stdout_lines,
                    stderr: result.stderr_lines,
                    error: result.error.unwrap_or_else(|| "Unknown error".to_string()),
                },
            ))
        }
    }
}
