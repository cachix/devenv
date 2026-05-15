//! Shell hot-reload integration for devenv.
//!
//! This module provides integration with devenv-reload to enable automatic
//! shell reloading when configuration files change.
//!
//! Watch files are populated from the eval cache during each build - the same
//! inputs that were tracked during Nix evaluation. This ensures we always watch
//! the files from the current evaluation, not stale data from previous sessions.

pub mod owner;

use crate::devenv::{format_shell_exports, resolve_shell_path};
use devenv_core::config::Clean;
use devenv_reload::{BuildContext, BuildError, CommandBuilder, ShellBuilder};
use devenv_shell::dialect::{BashDialect, RcfileContext, ShellDialect, create_dialect};
use owner::DevenvClient;
use std::collections::BTreeMap;

/// Builds shell commands for the reload coordinator.
///
/// `initial_env_script`, `bash_path`, and enterShell task exports are
/// pre-computed by the caller so the `Configuring shell` activity is
/// visible during the first build and not re-emitted on reload.
/// `devenv` routes Devenv calls to the owner actor.
pub struct DevenvShellBuilder {
    pub devenv: DevenvClient,
    pub cmd: Option<String>,
    pub args: Vec<String>,
    pub initial_env_script: String,
    pub bash_path: String,
    pub clean: Clean,
    pub dotfile: std::path::PathBuf,
    pub task_exports: BTreeMap<String, String>,
    pub task_messages: Vec<String>,
    pub shell: String,
}

impl ShellBuilder for DevenvShellBuilder {
    fn build(&self, ctx: &BuildContext) -> Result<CommandBuilder, BuildError> {
        // Interactive shell: reuse the pre-computed env script.
        // `get_dev_environment` is wrapped in `#[instrument_activity]`; we want
        // its progress visible on the very first run, so it ran inside
        // `run_reload_shell` while the TUI was still rendering. Calling it
        // again from here would re-evaluate the shell with no extra signal to
        // the user.
        if self.cmd.is_none() {
            let bash = &self.bash_path;

            // Write the devenv environment script to a file, appending task exports
            // (e.g. VIRTUAL_ENV, PATH with venv) after the Nix shell env so they take precedence.
            let env_script_path = self.dotfile.join("shell-env.sh");
            let mut env_script = self.initial_env_script.clone();
            env_script.push_str(&format_shell_exports(&self.task_exports));
            env_script.push_str(&BashDialect.format_task_messages(&self.task_messages));
            std::fs::write(&env_script_path, &env_script)
                .map_err(|e| BuildError::new(format!("Failed to write env script: {}", e)))?;

            tracing::trace!("Shell setting: {:?}", self.shell);
            let dialect = create_dialect(&self.shell);
            let target_shell_path = if dialect.name() != "bash" {
                let path = resolve_shell_path(dialect.name());
                tracing::trace!("Resolved {} shell path: {}", dialect.name(), path);
                Some(path)
            } else {
                None
            };

            let env_diff_helpers = dialect.env_diff_helpers();

            let reload_hook = if let Some(ref reload_file) = ctx.reload_file {
                dialect.reload_hook(reload_file)
            } else {
                String::new()
            };

            let rcfile_ctx = RcfileContext {
                env_script_path: &env_script_path,
                env_diff_helpers,
                reload_hook: &reload_hook,
                target_shell_path: target_shell_path.as_deref(),
                init_dir: &self.dotfile,
            };

            let rcfile_content = dialect.rcfile_content(&rcfile_ctx);

            dialect
                .write_init_files(&rcfile_ctx)
                .map_err(|e| BuildError::new(format!("Failed to write init files: {}", e)))?;

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

            cmd_builder.cwd(&ctx.cwd);

            if let Some(ref reload_file) = ctx.reload_file {
                cmd_builder.env(
                    "DEVENV_RELOAD_FILE",
                    reload_file.to_string_lossy().to_string(),
                );
            }

            let shell_for_env = target_shell_path.as_deref().unwrap_or(bash);
            crate::shell_env::apply_shell_env(&mut cmd_builder, shell_for_env, &self.clean);

            self.devenv.add_watch_paths_blocking(ctx.watcher.clone());

            return Ok(cmd_builder);
        }

        // Command mode: route prepare_exec through the owner task.
        let shell_config = self
            .devenv
            .prepare_exec_blocking(self.cmd.clone(), self.args.clone())?;

        let std_cmd = shell_config.command;
        let program = std_cmd.get_program().to_string_lossy().to_string();
        let args: Vec<String> = std_cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        let mut cmd_builder = CommandBuilder::new(&program);

        for arg in &args {
            cmd_builder.arg(arg);
        }

        if let Some(cwd) = std_cmd.get_current_dir() {
            cmd_builder.cwd(cwd);
        } else {
            cmd_builder.cwd(&ctx.cwd);
        }

        for (key, value) in std_cmd.get_envs() {
            if let Some(val) = value {
                cmd_builder.env(
                    key.to_string_lossy().to_string(),
                    val.to_string_lossy().to_string(),
                );
            }
        }

        if let Some(ref reload_file) = ctx.reload_file {
            cmd_builder.env(
                "DEVENV_RELOAD_FILE",
                reload_file.to_string_lossy().to_string(),
            );
        }

        self.devenv.add_watch_paths_blocking(ctx.watcher.clone());

        Ok(cmd_builder)
    }

    fn build_reload_env(&self, ctx: &BuildContext) -> Result<(), BuildError> {
        let reload_file = ctx
            .reload_file
            .as_ref()
            .ok_or_else(|| BuildError::new("reload_file not set in BuildContext"))?;

        self.devenv
            .build_reload_env_blocking(reload_file.clone(), ctx.watcher.clone())
    }

    fn interrupt(&self) {
        devenv_nix_backend::trigger_interrupt();
    }
}
