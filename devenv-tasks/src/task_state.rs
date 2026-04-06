use crate::SudoContext;
use crate::config::TaskConfig;
use crate::executor::{ExecutionContext, OutputCallback};
use crate::task_cache::{TaskCache, expand_glob_patterns};
use crate::types::{
    Output, Outputs, ProcessPhase, ProcessTaskStatus, Skipped, TaskCompleted, TaskFailure,
    TaskStatus, TaskType, VerbosityLevel, get_or_create_devenv_env_mut, process_name,
};
use base64::Engine;
use devenv_activity::{Activity, ActivityInstrument, ActivityLevel};
use devenv_processes::{NativeProcessManager, ProcessConfig};
use miette::{IntoDiagnostic, Result, WrapErr};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

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

/// Info returned from `run_process` about how the process was launched.
pub struct ProcessLaunchInfo {
    /// Whether the process has auto start off (start.enable = false).
    pub auto_start_off: bool,
    /// Whether the process has a readiness probe that must be awaited.
    pub requires_ready_wait: bool,
    /// The process manager name (stripped `devenv:processes:` prefix).
    pub process_name: String,
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
        let status = match task.r#type {
            TaskType::Process => TaskStatus::Process(ProcessTaskStatus {
                name: process_name(&task.name).to_string(),
                phase: ProcessPhase::Waiting,
            }),
            _ => TaskStatus::Pending,
        };
        Self {
            task,
            status,
            verbosity,
            sudo_context,
        }
    }

    /// Validate that the working directory exists and is a directory.
    fn validate_cwd(&self) -> Result<()> {
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
        Ok(())
    }

    /// Try to get cached output for this task, logging any errors.
    async fn get_cached_output(&self, cache: &TaskCache) -> Option<serde_json::Value> {
        match cache.get_task_output(&self.task.name).await {
            Ok(Some(output)) => {
                tracing::debug!(
                    "Found cached output for task {} in database",
                    self.task.name
                );
                Some(output)
            }
            Ok(None) => {
                tracing::debug!("No cached output found for task {}", self.task.name);
                None
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to get cached output for task {}: {}",
                    self.task.name,
                    e
                );
                None
            }
        }
    }

    /// Handle file modification checking with centralized error handling.
    /// Returns a Result with a boolean indicating if files were modified.
    #[tracing::instrument(
        name = "exec_if_modified",
        skip(self, cache),
        fields(
            task.name = %self.task.name,
            task.cached,
            exec_if_modified.pattern_count,
            exec_if_modified.include_pattern_count,
            exec_if_modified.exclude_pattern_count,
            exec_if_modified.matched_file_count,
        )
    )]
    async fn check_files_modified_result(
        &self,
        cache: &TaskCache,
    ) -> Result<bool, devenv_cache_core::error::CacheError> {
        if self.task.exec_if_modified.is_empty() {
            return Ok(false);
        }

        let patterns = &self.task.exec_if_modified;
        let include_count = patterns.iter().filter(|p| !p.starts_with('!')).count();
        let exclude_count = patterns.len() - include_count;
        let matched_files = expand_glob_patterns(patterns);

        let span = tracing::Span::current();
        span.record("exec_if_modified.pattern_count", patterns.len());
        span.record("exec_if_modified.include_pattern_count", include_count);
        span.record("exec_if_modified.exclude_pattern_count", exclude_count);
        span.record("exec_if_modified.matched_file_count", matched_files.len());

        let patterns_modified = cache
            .check_modified_files(&self.task.name, &self.task.exec_if_modified)
            .await?;
        if patterns_modified {
            span.record("task.cached", false);
            return Ok(true);
        }

        // Track command path changes separately so negation patterns in exec_if_modified
        // don't suppress cache invalidation for the task script itself.
        if let Some(cmd) = &self.task.command {
            let cmd_modified = cache
                .check_modified_files(&self.task.name, std::slice::from_ref(cmd))
                .await?;
            if cmd_modified {
                span.record("task.cached", false);
                return Ok(true);
            }
        }

        // Check for files previously tracked in the DB that no longer match the globs
        // (deleted, renamed, or moved outside the pattern). Build the full set of
        // currently expected paths so we don't false-positive on command paths.
        let mut all_current_paths = expand_glob_patterns(&self.task.exec_if_modified);
        if let Some(cmd) = &self.task.command {
            all_current_paths.push(cmd.clone());
        }
        let removed = cache
            .has_removed_files(&self.task.name, &all_current_paths)
            .await?;

        span.record("task.cached", !removed);
        Ok(removed)
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
    fn prepare_env(
        &self,
        outputs: &Outputs,
        shell_env: &std::collections::HashMap<String, String>,
    ) -> Result<BTreeMap<String, String>> {
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

        // Set environment variables from task outputs
        let env_exports = outputs.collect_env_exports();
        let mut devenv_env = String::new();
        for (env_key, env_str) in &env_exports {
            devenv_env.push_str(&format!(
                "export {}={}\n",
                env_key,
                shell_escape::escape(std::borrow::Cow::Borrowed(env_str))
            ));
        }
        env.extend(env_exports);
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

        Ok(env)
    }

    /// Create a temporary file for task I/O.
    fn create_tempfile(prefix: &str, suffix: &str) -> Result<tempfile::NamedTempFile> {
        tempfile::Builder::new()
            .prefix(prefix)
            .suffix(suffix)
            .tempfile()
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to create temporary file ({prefix})"))
    }

    async fn get_outputs(
        outputs_file: &tempfile::NamedTempFile,
        exports_file: &tempfile::NamedTempFile,
        stdout_lines: &[(std::time::Instant, String)],
    ) -> Output {
        // Read both files concurrently
        let (output_data, export_data) = tokio::join!(
            tokio::fs::read(outputs_file.path()),
            tokio::fs::read(exports_file.path()),
        );

        // TODO: report JSON parsing errors
        let mut output: Option<serde_json::Value> = output_data
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok());

        // Collect exports from both the legacy stdout protocol (pre-2.0.4 Nix modules)
        // and the file based protocol (CLI 2.0.4+). File exports are applied last
        // so they take precedence over stdout exports.
        let stdout_exports = Self::parse_stdout_exports(stdout_lines);
        let file_exports = match export_data {
            Ok(data) if !data.is_empty() => Self::parse_exports(&data),
            _ => Vec::new(),
        };

        if !stdout_exports.is_empty() || !file_exports.is_empty() {
            let out = output.get_or_insert_with(|| serde_json::json!({}));
            if let Some(env_obj) = get_or_create_devenv_env_mut(out) {
                for (k, v) in stdout_exports.into_iter().chain(file_exports) {
                    env_obj.insert(k, serde_json::Value::String(v));
                }
            } else {
                tracing::warn!(
                    "Task output is not a JSON object, {} export(s) dropped",
                    stdout_exports.len() + file_exports.len()
                );
            }
        }

        Output(output)
    }

    /// Decode base64 bytes into a UTF-8 string, logging a warning on failure.
    fn decode_b64(data: &[u8], context: &str) -> Option<String> {
        match B64.decode(data) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(s) => Some(s),
                Err(e) => {
                    tracing::warn!("Skipping {context} with invalid UTF-8: {e}");
                    None
                }
            },
            Err(e) => {
                tracing::warn!("Skipping {context} with invalid base64: {e}");
                None
            }
        }
    }

    /// Parse DEVENV_EXPORT lines from stdout (legacy protocol for pre-2.0.4 Nix modules).
    /// Format: DEVENV_EXPORT:<base64-key>=<base64-value>
    fn parse_stdout_exports(
        stdout_lines: &[(std::time::Instant, String)],
    ) -> Vec<(String, String)> {
        let mut exports = Vec::new();
        for (_, line) in stdout_lines {
            if let Some(rest) = line.strip_prefix("DEVENV_EXPORT:") {
                // Base64 uses '=' for padding, so find the separator '=' at the
                // first position that is a multiple of 4 (end of a valid base64 string).
                let split_pos = (4..rest.len())
                    .step_by(4)
                    .find(|&i| rest.as_bytes()[i] == b'=');
                if let Some(pos) = split_pos {
                    if let (Some(var), Some(val)) = (
                        Self::decode_b64(rest[..pos].as_bytes(), "DEVENV_EXPORT key"),
                        Self::decode_b64(rest[pos + 1..].as_bytes(), "DEVENV_EXPORT value"),
                    ) {
                        exports.push((var, val));
                    }
                }
            }
        }
        exports
    }

    /// Parse null-separated name\0base64(value)\0 pairs from exports file.
    fn parse_exports(data: &[u8]) -> Vec<(String, String)> {
        let mut exports = Vec::new();
        let mut parts = data.split(|&b| b == 0);
        while let (Some(name), Some(value_b64)) = (parts.next(), parts.next()) {
            if !name.is_empty() {
                let name_str = match std::str::from_utf8(name) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("Skipping export with invalid UTF-8 name: {e}");
                        continue;
                    }
                };
                if let Some(value) = Self::decode_b64(value_b64, name_str) {
                    exports.push((name_str.to_string(), value));
                }
            }
        }
        exports
    }

    /// Build a `ProcessConfig` from this task's config, merging environment variables.
    ///
    /// The process name is derived by stripping the `devenv:processes:` prefix
    /// from the task name (which all process tasks are expected to have).
    pub fn build_process_config(
        &self,
        env: &std::collections::HashMap<String, String>,
        bash: &str,
    ) -> Result<ProcessConfig> {
        let cmd = self
            .task
            .command
            .as_ref()
            .ok_or_else(|| miette::miette!("Process task {} has no command", self.task.name))?;

        let process_name = process_name(&self.task.name).to_string();

        let base = self.task.process.clone().unwrap_or_default();
        let mut config = ProcessConfig {
            name: process_name,
            exec: cmd.clone(),
            cwd: self.task.cwd.clone().map(std::path::PathBuf::from),
            ..base
        };

        // Merge devenv shell environment into process config
        // Task-level env takes precedence over shell env,
        // process-specific env takes precedence over both
        let mut merged_env = env.clone();
        merged_env.extend(self.task.env.clone());
        merged_env.extend(config.env.clone());
        config.env = merged_env;
        config.bash = bash.to_string();

        Ok(config)
    }

    /// Launch a process task and return info about how it was launched.
    ///
    /// This spawns a process using NativeProcessManager but does not wait for
    /// readiness or set task status. The caller is responsible for status tracking.
    pub async fn run_process(
        &self,
        manager: &Arc<NativeProcessManager>,
        config: ProcessConfig,
    ) -> Result<ProcessLaunchInfo> {
        tracing::info!("Launching process task: {}", self.task.name);

        let requires_ready_wait = config.has_readiness_probe();
        let process_name = config.name.clone();

        // Launch the pre-registered waiting process.
        let started = manager.launch_waiting(&config.name).await?;

        let auto_start_off = started.is_none();
        if auto_start_off {
            tracing::info!("Process task {} has auto start off", self.task.name);
        }

        Ok(ProcessLaunchInfo {
            auto_start_off,
            requires_ready_wait,
            process_name,
        })
    }

    /// Run this task with a pre-assigned activity ID.
    /// The Task::Hierarchy event has already been emitted; this emits Task::Start.
    pub async fn run(
        &self,
        now: Instant,
        outputs: &Outputs,
        cache: &TaskCache,
        cancellation: CancellationToken,
        activity_id: u64,
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
            refresh_task_cache,
            shell_env,
        )
        .in_activity(&task_activity)
        .await
    }

    async fn run_inner(
        &self,
        now: Instant,
        outputs: &Outputs,
        cache: &TaskCache,
        cancellation: CancellationToken,
        task_activity: &Activity,
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
                let cached_output = self.get_cached_output(cache).await;

                self.validate_cwd()?;
                let env = self
                    .prepare_env(outputs, shell_env)
                    .wrap_err("Failed to prepare status command")?;
                let exports_file = Self::create_tempfile("devenv_task_exports", "")?;
                let ctx = ExecutionContext {
                    command: cmd,
                    cwd: self.task.cwd.as_deref(),
                    env,
                    use_sudo: self.sudo_context.is_some(),
                    output_file_path: std::path::Path::new("/dev/null"),
                    exports_file_path: exports_file.path(),
                };
                let mut command = ctx.build_command();

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
                            // Start with cached output, merge in any exports from the status command
                            let mut result = cached_output.unwrap_or_else(|| serde_json::json!({}));
                            if let Ok(data) = tokio::fs::read(exports_file.path()).await {
                                let exports = Self::parse_exports(&data);
                                if let (false, Some(env_obj)) = (
                                    exports.is_empty(),
                                    get_or_create_devenv_env_mut(&mut result),
                                ) {
                                    for (k, v) in exports {
                                        env_obj.insert(k, serde_json::Value::String(v));
                                    }
                                }
                            }
                            let output = Output(Some(result));
                            tracing::debug!(
                                "Task {} skipped with output: {:?}",
                                self.task.name,
                                output
                            );
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
                    // First check if we have outputs in the current run's outputs map,
                    // then fall back to the cache
                    let task_output = match outputs.get(&self.task.name).cloned() {
                        Some(output) => Some(output),
                        None => self.get_cached_output(cache).await,
                    };

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

        self.validate_cwd()?;

        // Prepare environment
        let env = self
            .prepare_env(outputs, shell_env)
            .wrap_err("Failed to prepare task environment")?;

        // Create temporary files for task output and exports
        let outputs_file = Self::create_tempfile("devenv_task_output", ".json")?;
        let exports_file = Self::create_tempfile("devenv_task_exports", "")?;

        // Build execution context
        let ctx = ExecutionContext {
            command: cmd,
            cwd: self.task.cwd.as_deref(),
            env,
            use_sudo: self.sudo_context.is_some(),
            output_file_path: outputs_file.path(),
            exports_file_path: exports_file.path(),
        };

        // Execute using the provided executor
        let callback = ActivityCallback::new(task_activity);
        let result = crate::executor::execute(ctx, &callback, cancellation).await;

        // Only update file states on success - failed tasks should not be cached
        if result.success {
            let expanded_paths = expand_glob_patterns(&self.task.exec_if_modified);
            for path in &expanded_paths {
                cache.update_file_state(&self.task.name, path).await?;
            }
            cache
                .cleanup_stale_files(&self.task.name, &expanded_paths)
                .await?;

            if let Some(cmd) = &self.task.command {
                cache.update_file_state(&self.task.name, cmd).await?;
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
                Self::get_outputs(&outputs_file, &exports_file, &result.stdout_lines).await,
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

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use proptest::prelude::*;
    use std::time::Instant;

    fn encode(s: &str) -> String {
        B64.encode(s)
    }

    fn make_line(key: &str, value: &str) -> (Instant, String) {
        (
            Instant::now(),
            format!("DEVENV_EXPORT:{}={}", encode(key), encode(value)),
        )
    }

    fn make_file_data(pairs: &[(&str, &str)]) -> Vec<u8> {
        let mut data = Vec::new();
        for (name, value) in pairs {
            data.extend_from_slice(name.as_bytes());
            data.push(0);
            data.extend_from_slice(B64.encode(value).as_bytes());
            data.push(0);
        }
        data
    }

    // -- parse_exports tests --

    #[test]
    fn parse_exports_empty() {
        assert!(TaskState::parse_exports(b"").is_empty());
    }

    #[test]
    fn parse_exports_single() {
        let data = make_file_data(&[("FOO", "bar")]);
        let result = TaskState::parse_exports(&data);
        assert_eq!(result, vec![("FOO".into(), "bar".into())]);
    }

    #[test]
    fn parse_exports_multiple() {
        let data = make_file_data(&[("A", "1"), ("B", "2"), ("C", "3")]);
        let result = TaskState::parse_exports(&data);
        assert_eq!(
            result,
            vec![
                ("A".into(), "1".into()),
                ("B".into(), "2".into()),
                ("C".into(), "3".into()),
            ]
        );
    }

    #[test]
    fn parse_exports_empty_value() {
        let data = make_file_data(&[("KEY", "")]);
        let result = TaskState::parse_exports(&data);
        assert_eq!(result, vec![("KEY".into(), String::new())]);
    }

    #[test]
    fn parse_exports_value_with_special_chars() {
        let data = make_file_data(&[("P", "hello world"), ("Q", "a=b=c"), ("R", "line\nnewline")]);
        let result = TaskState::parse_exports(&data);
        assert_eq!(
            result,
            vec![
                ("P".into(), "hello world".into()),
                ("Q".into(), "a=b=c".into()),
                ("R".into(), "line\nnewline".into()),
            ]
        );
    }

    #[test]
    fn parse_exports_skips_empty_name() {
        // Manually craft data with an empty name: \0<base64>\0
        let mut data = Vec::new();
        data.push(0);
        data.extend_from_slice(B64.encode("val").as_bytes());
        data.push(0);
        // Then a valid pair
        data.extend_from_slice(b"GOOD");
        data.push(0);
        data.extend_from_slice(B64.encode("ok").as_bytes());
        data.push(0);

        let result = TaskState::parse_exports(&data);
        assert_eq!(result, vec![("GOOD".into(), "ok".into())]);
    }

    #[test]
    fn parse_exports_invalid_base64_skipped() {
        let mut data = Vec::new();
        data.extend_from_slice(b"NAME");
        data.push(0);
        data.extend_from_slice(b"!!!not-base64!!!");
        data.push(0);

        let result = TaskState::parse_exports(&data);
        assert!(result.is_empty());
    }

    #[test]
    fn parse_exports_odd_field_ignored() {
        // A trailing name without a value pair is ignored
        let mut data = make_file_data(&[("A", "1")]);
        data.extend_from_slice(b"ORPHAN");
        let result = TaskState::parse_exports(&data);
        assert_eq!(result, vec![("A".into(), "1".into())]);
    }

    // -- parse_stdout_exports tests --

    #[test]
    fn parse_stdout_exports_empty() {
        assert!(TaskState::parse_stdout_exports(&[]).is_empty());
    }

    #[test]
    fn parse_stdout_exports_ignores_non_export_lines() {
        let lines = vec![
            (Instant::now(), "some normal output".into()),
            (Instant::now(), "building stuff...".into()),
        ];
        assert!(TaskState::parse_stdout_exports(&lines).is_empty());
    }

    #[test]
    fn parse_stdout_exports_single() {
        let lines = vec![make_line("MY_VAR", "my_value")];
        let result = TaskState::parse_stdout_exports(&lines);
        assert_eq!(result, vec![("MY_VAR".into(), "my_value".into())]);
    }

    #[test]
    fn parse_stdout_exports_mixed_lines() {
        let lines = vec![
            (Instant::now(), "before".into()),
            make_line("X", "1"),
            (Instant::now(), "middle".into()),
            make_line("Y", "2"),
            (Instant::now(), "after".into()),
        ];
        let result = TaskState::parse_stdout_exports(&lines);
        assert_eq!(
            result,
            vec![("X".into(), "1".into()), ("Y".into(), "2".into())]
        );
    }

    #[test]
    fn parse_stdout_exports_short_key() {
        // 1-char key "A" -> base64 "QQ==" (4 chars with padding)
        let lines = vec![make_line("A", "val")];
        let result = TaskState::parse_stdout_exports(&lines);
        assert_eq!(result, vec![("A".into(), "val".into())]);
    }

    #[test]
    fn parse_stdout_exports_empty_value() {
        let lines = vec![make_line("KEY", "")];
        let result = TaskState::parse_stdout_exports(&lines);
        assert_eq!(result, vec![("KEY".into(), String::new())]);
    }

    #[test]
    fn parse_stdout_exports_value_with_equals() {
        let lines = vec![make_line("PATH", "/usr/bin:/bin")];
        let result = TaskState::parse_stdout_exports(&lines);
        assert_eq!(result, vec![("PATH".into(), "/usr/bin:/bin".into())]);
    }

    // -- proptest round-trip tests --

    proptest! {
        #[test]
        fn parse_exports_roundtrip(pairs in prop::collection::vec(("[A-Za-z_][A-Za-z0-9_]{0,30}", ".*"), 0..20)) {
            let refs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let data = make_file_data(&refs);
            let result = TaskState::parse_exports(&data);
            prop_assert_eq!(result, pairs);
        }

        #[test]
        fn parse_stdout_exports_roundtrip(pairs in prop::collection::vec(("[A-Za-z_][A-Za-z0-9_]{0,30}", ".*"), 0..20)) {
            let lines: Vec<(Instant, String)> = pairs.iter().map(|(k, v)| make_line(k, v)).collect();
            let result = TaskState::parse_stdout_exports(&lines);
            prop_assert_eq!(result, pairs);
        }
    }
}
