//! Shell hot-reload integration for devenv.
//!
//! This module provides integration with devenv-reload to enable automatic
//! shell reloading when configuration files change.
//!
//! Watch files are populated from the eval cache during each build - the same
//! inputs that were tracked during Nix evaluation. This ensures we always watch
//! the files from the current evaluation, not stale data from previous sessions.

use crate::Devenv;
use devenv_reload::{BuildContext, BuildError, CommandBuilder, ShellBuilder};
use devenv_shell::dialect::{BashDialect, RcfileContext, ShellDialect};
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::Mutex;

/// Shell builder that evaluates devenv environment on each build.
pub struct DevenvShellBuilder {
    /// Tokio runtime handle for running async code in sync context
    handle: Handle,
    /// Devenv instance wrapped in async mutex
    devenv: Arc<Mutex<Devenv>>,
    /// Optional command to run (None for interactive shell)
    cmd: Option<String>,
    /// Arguments for the command
    args: Vec<String>,
    /// Pre-computed environment script (from print_dev_env, computed before coordinator starts)
    initial_env_script: String,
    /// Pre-computed bash path
    bash_path: String,
    /// Dotfile directory path
    dotfile: std::path::PathBuf,
    /// Eval cache pool (from original devenv, to query file inputs for watching)
    eval_cache_pool: Option<sqlx::SqlitePool>,
    /// Shell cache key (from original devenv, to query file inputs for watching)
    shell_cache_key: Option<devenv_eval_cache::EvalCacheKey>,
}

impl DevenvShellBuilder {
    /// Create a new DevenvShellBuilder.
    ///
    /// The provided Devenv instance will be used for all builds.
    /// The `initial_env_script` and `bash_path` are pre-computed while TUI is active
    /// to avoid deadlocks (get_dev_environment has #[activity] which needs TUI).
    /// The `eval_cache_pool` and `shell_cache_key` are from the original devenv
    /// (needed because the new devenv instance hasn't done assemble() yet).
    pub fn new(
        handle: Handle,
        devenv: Arc<Mutex<Devenv>>,
        cmd: Option<String>,
        args: Vec<String>,
        initial_env_script: String,
        bash_path: String,
        dotfile: std::path::PathBuf,
        eval_cache_pool: Option<sqlx::SqlitePool>,
        shell_cache_key: Option<devenv_eval_cache::EvalCacheKey>,
    ) -> Self {
        Self {
            handle,
            devenv,
            cmd,
            args,
            initial_env_script,
            bash_path,
            dotfile,
            eval_cache_pool,
            shell_cache_key,
        }
    }
}

impl ShellBuilder for DevenvShellBuilder {
    fn build(&self, ctx: &BuildContext) -> Result<CommandBuilder, BuildError> {
        // For interactive shell, use pre-computed env script.
        // NOTE: We use pre-computed values because get_dev_environment has #[activity]
        // which needs TUI, but TUI waits for this build() to complete = deadlock.
        // The env script was computed in run_reload_shell() while TUI was active.
        if self.cmd.is_none() {
            let bash = &self.bash_path;

            // Write the devenv environment script to a file
            let env_script_path = self.dotfile.join("shell-env.sh");
            std::fs::write(&env_script_path, &self.initial_env_script)
                .map_err(|e| BuildError::new(format!("Failed to write env script: {}", e)))?;

            let dialect = BashDialect;
            let env_diff_helpers = dialect.env_diff_helpers();

            let reload_hook = if let Some(ref reload_file) = ctx.reload_file {
                dialect.reload_hook(reload_file)
            } else {
                String::new()
            };

            let rcfile_content = dialect.rcfile_content(&RcfileContext {
                env_script_path: &env_script_path,
                env_diff_helpers,
                reload_hook: &reload_hook,
            });

            let rcfile_path = self.dotfile.join("shell-rcfile.sh");
            std::fs::write(&rcfile_path, &rcfile_content)
                .map_err(|e| BuildError::new(format!("Failed to write rcfile: {}", e)))?;

            let interactive_args = dialect.interactive_args();
            let mut cmd_builder = CommandBuilder::new(bash);
            for arg in &interactive_args.prefix {
                cmd_builder.arg(arg);
            }
            cmd_builder.arg(rcfile_path.to_string_lossy().as_ref());
            for arg in &interactive_args.suffix {
                cmd_builder.arg(arg);
            }

            // Set working directory
            cmd_builder.cwd(&ctx.cwd);

            // Set DEVENV_RELOAD_FILE for any scripts that need it
            if let Some(ref reload_file) = ctx.reload_file {
                cmd_builder.env(
                    "DEVENV_RELOAD_FILE",
                    reload_file.to_string_lossy().to_string(),
                );
            }

            // Add watch paths from eval cache
            self.handle.block_on(async {
                self.add_watch_paths_from_cache(ctx).await;
            });

            return Ok(cmd_builder);
        }

        // For command mode, use prepare_exec which handles the script execution
        // This still needs async since command mode may need different env
        self.handle.block_on(async {
            let devenv = self.devenv.lock().await;

            let shell_config = devenv
                .prepare_exec(self.cmd.clone(), &self.args)
                .await
                .map_err(|e| BuildError::new(format!("Failed to prepare shell: {}", e)))?;

            // Convert std::process::Command to portable_pty::CommandBuilder
            let std_cmd = shell_config.command;
            let program = std_cmd.get_program().to_string_lossy().to_string();
            let args: Vec<String> = std_cmd
                .get_args()
                .map(|a| a.to_string_lossy().to_string())
                .collect();

            let mut cmd_builder = CommandBuilder::new(&program);

            // Add arguments
            for arg in &args {
                cmd_builder.arg(arg);
            }

            // Set working directory
            if let Some(cwd) = std_cmd.get_current_dir() {
                cmd_builder.cwd(cwd);
            } else {
                cmd_builder.cwd(&ctx.cwd);
            }

            // Copy environment variables from the command
            for (key, value) in std_cmd.get_envs() {
                if let Some(val) = value {
                    cmd_builder.env(
                        key.to_string_lossy().to_string(),
                        val.to_string_lossy().to_string(),
                    );
                }
            }

            // Set DEVENV_RELOAD_FILE for the PROMPT_COMMAND hook
            if let Some(ref reload_file) = ctx.reload_file {
                cmd_builder.env(
                    "DEVENV_RELOAD_FILE",
                    reload_file.to_string_lossy().to_string(),
                );
            }

            // Add watch paths from eval cache
            self.add_watch_paths_from_cache(ctx).await;

            Ok(cmd_builder)
        })
    }

    fn build_reload_env(&self, ctx: &BuildContext) -> Result<(), BuildError> {
        let reload_file = ctx
            .reload_file
            .as_ref()
            .ok_or_else(|| BuildError::new("reload_file not set in BuildContext"))?;

        // Create a dedicated runtime for this operation to avoid panics during main runtime shutdown.
        // This is called from spawn_blocking when file changes are detected.
        // If the main runtime is shutting down (shell exited), we want to gracefully fail
        // rather than panic.
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                return Err(BuildError::new(format!(
                    "Failed to create runtime for reload build: {}",
                    e
                )));
            }
        };

        let devenv = self.devenv.clone();
        let reload_file = reload_file.clone();
        let watcher = ctx.watcher.clone();
        let eval_cache_pool = self.eval_cache_pool.clone();
        let shell_cache_key = self.shell_cache_key.clone();

        rt.block_on(async move {
            let devenv = devenv.lock().await;

            // Invalidate cached state to force re-evaluation on file changes
            devenv.invalidate_for_reload();

            // Get the bash environment script (like direnv's print-dev-env)
            let env_script = devenv
                .print_dev_env(false)
                .await
                .map_err(|e| BuildError::new(format!("Failed to build environment: {}", e)))?;

            // Write atomically: write to temp file, then rename
            let temp_path = reload_file.with_extension("sh.tmp");
            std::fs::write(&temp_path, &env_script)
                .map_err(|e| BuildError::new(format!("Failed to write pending env: {}", e)))?;
            std::fs::rename(&temp_path, &reload_file)
                .map_err(|e| BuildError::new(format!("Failed to rename pending env: {}", e)))?;

            // Add watch paths from eval cache for the new inputs
            if let (Some(pool), Some(cache_key)) = (&eval_cache_pool, &shell_cache_key) {
                match devenv_eval_cache::get_file_inputs_by_key_hash(pool, &cache_key.key_hash)
                    .await
                {
                    Ok(inputs) => {
                        for input in inputs {
                            if input.path.exists() && !input.path.starts_with("/nix/store") {
                                watcher.watch(&input.path);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to query eval cache for shell inputs: {}", e);
                    }
                }
            }

            Ok(())
        })
    }
}

impl DevenvShellBuilder {
    /// Add watch paths from the eval cache to the watcher.
    /// This watches exactly the files that were inputs to shell evaluation.
    /// Uses stored pool and cache key since the devenv instance may not have them set.
    async fn add_watch_paths_from_cache(&self, ctx: &BuildContext) {
        let Some(pool) = &self.eval_cache_pool else {
            tracing::debug!("No eval cache pool available");
            return;
        };

        // First try: get files by specific shell cache key
        if let Some(cache_key) = &self.shell_cache_key {
            tracing::debug!(
                "Looking up file inputs for key_hash: {}",
                cache_key.key_hash
            );
            match devenv_eval_cache::get_file_inputs_by_key_hash(pool, &cache_key.key_hash).await {
                Ok(inputs) if !inputs.is_empty() => {
                    tracing::debug!("Found {} file inputs for shell key", inputs.len());
                    for input in inputs {
                        if input.path.exists() && !input.path.starts_with("/nix/store") {
                            ctx.watcher.watch(&input.path);
                        }
                    }
                    return;
                }
                Ok(_) => {
                    tracing::debug!("No file inputs found for shell key, trying all tracked files");
                }
                Err(e) => {
                    tracing::warn!("Failed to query by key_hash: {}", e);
                }
            }
        }

        // Fallback: get all tracked files from any evaluation
        match devenv_eval_cache::get_all_tracked_file_paths(pool).await {
            Ok(paths) => {
                tracing::debug!("Found {} total tracked files in eval cache", paths.len());
                for path in paths {
                    if path.exists() && !path.starts_with("/nix/store") {
                        ctx.watcher.watch(&path);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to query all tracked files: {}", e);
            }
        }
    }
}
