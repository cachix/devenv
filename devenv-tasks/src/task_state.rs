use crate::SudoContext;
use crate::config::TaskConfig;
use crate::executor::{ExecutionContext, OutputCallback, SubprocessExecutor};
use crate::task_cache::{ModifiedFilesCheck, TaskCache, expand_glob_patterns};
use crate::types::{Output, Skipped, TaskCompleted, TaskFailure, TaskStatus, VerbosityLevel};
use devenv_activity::{Activity, ActivityInstrument, ActivityLevel};
use devenv_processes::{ListenKind, NativeProcessManager, ProcessConfig};
use miette::{IntoDiagnostic, Result, WrapErr};
use rand::RngCore;
use serde_json::json;
use std::collections::BTreeMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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

#[derive(Clone, Debug)]
struct ExecIfModifiedCheckResult {
    modified: bool,
    patterns: ModifiedFilesCheck,
    command_path: Option<ModifiedFilesCheck>,
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
    ) -> Result<ExecIfModifiedCheckResult, devenv_cache_core::error::CacheError> {
        if self.task.exec_if_modified.is_empty() {
            return Ok(ExecIfModifiedCheckResult {
                modified: false,
                patterns: ModifiedFilesCheck {
                    modified: false,
                    pattern_count: 0,
                    include_pattern_count: 0,
                    exclude_pattern_count: 0,
                    matched_file_count: 0,
                },
                command_path: None,
            });
        }

        let patterns = cache
            .check_modified_files_with_stats(&self.task.name, &self.task.exec_if_modified)
            .await?;
        if patterns.modified {
            return Ok(ExecIfModifiedCheckResult {
                modified: true,
                patterns,
                command_path: None,
            });
        }

        // Track command path changes separately so negation patterns in exec_if_modified
        // don't suppress cache invalidation for the task script itself.
        if let Some(cmd) = &self.task.command {
            let command_path = cache
                .check_modified_files_with_stats(&self.task.name, std::slice::from_ref(cmd))
                .await?;
            return Ok(ExecIfModifiedCheckResult {
                modified: command_path.modified,
                patterns,
                command_path: Some(command_path),
            });
        }

        Ok(ExecIfModifiedCheckResult {
            modified: false,
            patterns,
            command_path: None,
        })
    }

    /// Check if any files specified in exec_if_modified have been modified.
    /// Returns true if any files have been modified or if there was an error checking.
    async fn check_modified_files(&self, cache: &TaskCache) -> bool {
        let started_at = SystemTime::now();
        let started_monotonic = Instant::now();
        match self.check_files_modified_result(cache).await {
            Ok(result) => {
                self.emit_exec_if_modified_status_span(
                    &result,
                    started_at,
                    started_monotonic.elapsed().as_millis(),
                )
                .await;
                result.modified
            }
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

    async fn emit_exec_if_modified_status_span(
        &self,
        result: &ExecIfModifiedCheckResult,
        started_at: SystemTime,
        eval_ms: u128,
    ) {
        if self.task.exec_if_modified.is_empty() {
            return;
        }

        if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_none()
            && std::env::var_os("OTEL_SPAN_SPOOL_DIR").is_none()
        {
            return;
        }

        let end_at = SystemTime::now();
        let start_ns = match started_at.duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_nanos().to_string(),
            Err(_) => return,
        };
        let end_ns = match end_at.duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_nanos().to_string(),
            Err(_) => return,
        };

        let mut trace_id = random_hex(16);
        let mut parent_span_id = None;
        if let Some(traceparent) =
            std::env::var_os("OTEL_TASK_TRACEPARENT").or_else(|| std::env::var_os("TRACEPARENT"))
            && let Some(parsed) = parse_traceparent(&traceparent.to_string_lossy())
        {
            trace_id = parsed.trace_id;
            parent_span_id = Some(parsed.parent_span_id);
        }

        let exit_code = if result.modified { 1 } else { 0 };
        let mut attributes = vec![
            json!({"key": "service.name", "value": {"stringValue": "dt-task"}}),
            json!({"key": "exit.code", "value": {"intValue": exit_code.to_string()}}),
            json!({"key": "task.phase", "value": {"stringValue": "status"}}),
            json!({"key": "status.method", "value": {"stringValue": "exec_if_modified"}}),
            json!({"key": "task.exec_if_modified.pattern_count", "value": {"intValue": result.patterns.pattern_count.to_string()}}),
            json!({"key": "task.exec_if_modified.include_pattern_count", "value": {"intValue": result.patterns.include_pattern_count.to_string()}}),
            json!({"key": "task.exec_if_modified.exclude_pattern_count", "value": {"intValue": result.patterns.exclude_pattern_count.to_string()}}),
            json!({"key": "task.exec_if_modified.matched_file_count", "value": {"intValue": result.patterns.matched_file_count.to_string()}}),
            json!({"key": "task.exec_if_modified.modified", "value": {"boolValue": result.modified}}),
            json!({"key": "task.exec_if_modified.eval_ms", "value": {"intValue": eval_ms.to_string()}}),
            json!({"key": "task.cached", "value": {"boolValue": !result.modified}}),
        ];

        if let Ok(devenv_root) = std::env::var("DEVENV_ROOT") {
            attributes.push(json!({"key": "devenv.root", "value": {"stringValue": devenv_root}}));
        }

        if let Some(command_path) = &result.command_path {
            attributes.push(
                json!({"key": "task.exec_if_modified.command_path_checked", "value": {"boolValue": true}}),
            );
            attributes.push(json!({
                "key": "task.exec_if_modified.command_path_matched_file_count",
                "value": {"intValue": command_path.matched_file_count.to_string()}
            }));
            attributes.push(json!({
                "key": "task.exec_if_modified.command_path_modified",
                "value": {"boolValue": command_path.modified}
            }));
        }

        let mut span = json!({
            "traceId": trace_id,
            "spanId": random_hex(8),
            "name": format!("{}:status", self.task.name),
            "kind": 1,
            "startTimeUnixNano": start_ns,
            "endTimeUnixNano": end_ns,
            "attributes": attributes,
            "status": {"code": 1},
        });
        if let Some(parent_span_id) = parent_span_id {
            span["parentSpanId"] = json!(parent_span_id);
        }

        let payload = json!({
            "resourceSpans": [{
                "resource": {
                    "attributes": [
                        {"key": "service.name", "value": {"stringValue": "dt-task"}}
                    ]
                },
                "scopeSpans": [{
                    "scope": {"name": "devenv-tasks"},
                    "spans": [span]
                }]
            }]
        });

        let mut command = Command::new("otel-span");
        command
            .arg("emit")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let Ok(mut child) = command.spawn() else {
            return;
        };

        if let Some(mut stdin) = child.stdin.take()
            && let Ok(payload) = serde_json::to_vec(&payload)
        {
            let _ = stdin.write_all(&payload).await;
        }

        if let Err(error) = child.wait().await {
            tracing::debug!(
                "Failed to emit exec_if_modified status span for task {}: {}",
                self.task.name,
                error
            );
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
        bash: &str,
        cancel: &tokio_util::sync::CancellationToken,
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

        // Merge devenv shell environment into process config
        // Task-level env takes precedence over shell env,
        // process-specific env takes precedence over both
        let mut merged_env = env.clone();
        merged_env.extend(self.task.env.clone());
        merged_env.extend(config.env.clone());
        config.env = merged_env;
        config.bash = bash.to_string();

        // Check if we need to wait for readiness (ready config, has listen sockets, or has allocated ports)
        let requires_ready_wait = config.ready.is_some()
            || config
                .listen
                .iter()
                .any(|spec| spec.kind == ListenKind::Tcp)
            || !config.ports.is_empty();

        // Start the process via the manager (which tracks it for shutdown).
        // Returns None for disabled processes (start.enable = false).
        let started = manager.start_command(&config, parent_id).await?;

        // Wait for ready signal if the process was actually started
        if started.is_some() && requires_ready_wait {
            tracing::info!("Waiting for process {} to signal ready...", self.task.name);
            manager.wait_ready(&config.name, cancel).await?;
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
                        && let (Ok(var), Ok(val)) =
                            (String::from_utf8(var_bytes), String::from_utf8(val_bytes))
                    {
                        exports.insert(var, serde_json::Value::String(val));
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
        if output.get("devenv").is_none() {
            output["devenv"] = serde_json::json!({});
        }
        if output["devenv"].get("env").is_none() {
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
        executor: &SubprocessExecutor,
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
        executor: &SubprocessExecutor,
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
            use_sudo: self.sudo_context.is_some(),
            output_file_path: outputs_file.path(),
        };

        // Execute using the provided executor
        let callback = ActivityCallback::new(task_activity);
        let result = executor.execute(ctx, &callback, cancellation).await;

        // Process any DEVENV_EXPORT lines from stdout and merge into output file
        Self::process_exports(&result.stdout_lines, &outputs_file).await;

        // Only update file states on success - failed tasks should not be cached
        if result.success {
            let expanded_paths = expand_glob_patterns(&self.task.exec_if_modified);
            for path in expanded_paths {
                cache.update_file_state(&self.task.name, &path).await?;
            }

            if let Some(cmd) = &self.task.command {
                for path in expand_glob_patterns(std::slice::from_ref(cmd)) {
                    cache.update_file_state(&self.task.name, &path).await?;
                }
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

struct TraceparentContext {
    trace_id: String,
    parent_span_id: String,
}

fn parse_traceparent(traceparent: &str) -> Option<TraceparentContext> {
    let mut parts = traceparent.split('-');
    let _version = parts.next()?;
    let trace_id = parts.next()?;
    let parent_span_id = parts.next()?;
    let _flags = parts.next()?;

    if trace_id.len() != 32 || parent_span_id.len() != 16 {
        return None;
    }

    Some(TraceparentContext {
        trace_id: trace_id.to_string(),
        parent_span_id: parent_span_id.to_string(),
    })
}

fn random_hex(byte_len: usize) -> String {
    let mut bytes = vec![0_u8; byte_len];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
