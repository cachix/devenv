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
}

impl DevenvShellBuilder {
    /// Create a new DevenvShellBuilder.
    ///
    /// The provided Devenv instance will be used for all builds.
    pub fn new(
        handle: Handle,
        devenv: Arc<Mutex<Devenv>>,
        cmd: Option<String>,
        args: Vec<String>,
    ) -> Self {
        Self {
            handle,
            devenv,
            cmd,
            args,
        }
    }
}

impl ShellBuilder for DevenvShellBuilder {
    fn build(&self, ctx: &BuildContext) -> Result<CommandBuilder, BuildError> {
        // Run async code in sync context
        self.handle.block_on(async {
            let devenv = self.devenv.lock().await;

            // Run enterShell tasks during build phase (TUI still active)
            let _ = devenv
                .run_enter_shell_tasks()
                .await
                .map_err(|e| BuildError::new(format!("enterShell tasks failed: {}", e)))?;

            // Prepare the shell command using prepare_exec which returns ShellCommand
            let shell_config = devenv
                .prepare_exec(self.cmd.clone(), &self.args)
                .await
                .map_err(|e| BuildError::new(format!("Failed to prepare shell: {}", e)))?;

            // Convert std::process::Command to portable_pty::CommandBuilder
            let std_cmd = shell_config.command;
            let program = std_cmd.get_program().to_string_lossy().to_string();
            let mut cmd_builder = CommandBuilder::new(program);

            // Add arguments
            for arg in std_cmd.get_args() {
                cmd_builder.arg(arg.to_string_lossy().to_string());
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

            // Skip task execution in shell (tasks already ran above)
            cmd_builder.env("DEVENV_SKIP_TASKS", "1");

            // Add watch paths from eval cache
            self.add_watch_paths_from_cache(&devenv, ctx).await;

            Ok(cmd_builder)
        })
    }

    fn build_reload_env(&self, ctx: &BuildContext) -> Result<(), BuildError> {
        let reload_file = ctx
            .reload_file
            .as_ref()
            .ok_or_else(|| BuildError::new("reload_file not set in BuildContext"))?;

        // Run async code in sync context
        self.handle.block_on(async {
            let devenv = self.devenv.lock().await;

            // Get the bash environment script (like direnv's print-dev-env)
            let env_script = devenv
                .print_dev_env(false)
                .await
                .map_err(|e| BuildError::new(format!("Failed to build environment: {}", e)))?;

            // Write atomically: write to temp file, then rename
            let temp_path = reload_file.with_extension("sh.tmp");
            std::fs::write(&temp_path, &env_script)
                .map_err(|e| BuildError::new(format!("Failed to write pending env: {}", e)))?;
            std::fs::rename(&temp_path, reload_file)
                .map_err(|e| BuildError::new(format!("Failed to rename pending env: {}", e)))?;

            // Add watch paths from eval cache for the new inputs
            self.add_watch_paths_from_cache(&devenv, ctx).await;

            Ok(())
        })
    }
}

impl DevenvShellBuilder {
    /// Add watch paths from the eval cache to the watcher.
    /// This watches exactly the files that were inputs to shell evaluation.
    async fn add_watch_paths_from_cache(&self, devenv: &Devenv, ctx: &BuildContext) {
        if let (Some(pool), Some(cache_key)) = (devenv.eval_cache_pool(), devenv.shell_cache_key())
        {
            match devenv_eval_cache::get_file_inputs_by_key_hash(pool, &cache_key.key_hash).await {
                Ok(inputs) => {
                    for input in inputs {
                        // Only watch files that exist and are not in /nix/store (immutable)
                        if input.path.exists() && !input.path.starts_with("/nix/store") {
                            let _ = ctx.watcher.watch(&input.path);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to query eval cache for shell inputs: {}", e);
                }
            }
        }
    }
}
