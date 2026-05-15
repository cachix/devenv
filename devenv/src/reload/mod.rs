//! Shell hot-reload integration for devenv.
//!
//! Watch files come from the eval cache after each build, so the watcher
//! always tracks the inputs of the current evaluation rather than stale data
//! from a previous session.

pub mod owner;

use crate::devenv::{format_shell_exports, resolve_shell_path};
use devenv_core::config::Clean;
use devenv_reload::{BuildContext, BuildError, CommandBuilder, ShellBuilder};
use devenv_shell::dialect::{BashDialect, RcfileContext, ShellDialect, create_dialect};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub use owner::{DevenvClient, spawn_owner};

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
    pub dotfile: PathBuf,
    pub task_exports: BTreeMap<String, String>,
    pub task_messages: Vec<String>,
    pub shell: String,
}

impl ShellBuilder for DevenvShellBuilder {
    fn build(&self, ctx: &BuildContext) -> Result<CommandBuilder, BuildError> {
        let cmd_builder = if self.cmd.is_none() {
            self.build_interactive(ctx)?
        } else {
            self.build_command(ctx)?
        };
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

impl DevenvShellBuilder {
    // The pre-computed env script is reused here. `get_dev_environment` is
    // wrapped in `#[instrument_activity]`; calling it from inside `build()`
    // would re-evaluate the shell after the TUI has already shut down,
    // emitting the activity to nothing.
    fn build_interactive(&self, ctx: &BuildContext) -> Result<CommandBuilder, BuildError> {
        let bash = &self.bash_path;

        // Append task exports after the Nix env so they take precedence
        // (e.g. VIRTUAL_ENV, PATH from venv override the Nix-provided ones).
        let env_script_path = self.dotfile.join("shell-env.sh");
        let mut env_script = self.initial_env_script.clone();
        env_script.push_str(&format_shell_exports(&self.task_exports));
        env_script.push_str(&BashDialect.format_task_messages(&self.task_messages));
        write_file(&env_script_path, &env_script, "env script")?;

        tracing::trace!("Shell setting: {:?}", self.shell);
        let dialect = create_dialect(&self.shell);
        let target_shell_path = (dialect.name() != "bash").then(|| {
            let path = resolve_shell_path(dialect.name());
            tracing::trace!("Resolved {} shell path: {}", dialect.name(), path);
            path
        });

        let reload_hook = ctx
            .reload_file
            .as_deref()
            .map(|f| dialect.reload_hook(f))
            .unwrap_or_default();

        let rcfile_ctx = RcfileContext {
            env_script_path: &env_script_path,
            env_diff_helpers: dialect.env_diff_helpers(),
            reload_hook: &reload_hook,
            target_shell_path: target_shell_path.as_deref(),
            init_dir: &self.dotfile,
        };

        dialect
            .write_init_files(&rcfile_ctx)
            .map_err(|e| BuildError::new(format!("Failed to write init files: {}", e)))?;

        let rcfile_path = self.dotfile.join("shell-rcfile.sh");
        write_file(&rcfile_path, &dialect.rcfile_content(&rcfile_ctx), "rcfile")?;

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
        set_reload_file_env(&mut cmd_builder, ctx);

        let shell_for_env = target_shell_path.as_deref().unwrap_or(bash);
        crate::shell_env::apply_shell_env(&mut cmd_builder, shell_for_env, &self.clean);

        Ok(cmd_builder)
    }

    fn build_command(&self, ctx: &BuildContext) -> Result<CommandBuilder, BuildError> {
        let shell_config = self
            .devenv
            .prepare_exec_blocking(self.cmd.clone(), self.args.clone())?;
        let std_cmd = shell_config.command;

        let mut cmd_builder = CommandBuilder::new(std_cmd.get_program().to_string_lossy().as_ref());
        for arg in std_cmd.get_args() {
            cmd_builder.arg(arg.to_string_lossy().as_ref());
        }

        cmd_builder.cwd(std_cmd.get_current_dir().unwrap_or(&ctx.cwd));

        for (key, value) in std_cmd.get_envs() {
            if let Some(val) = value {
                cmd_builder.env(
                    key.to_string_lossy().into_owned(),
                    val.to_string_lossy().into_owned(),
                );
            }
        }

        set_reload_file_env(&mut cmd_builder, ctx);

        Ok(cmd_builder)
    }
}

fn write_file(path: &Path, content: &str, what: &str) -> Result<(), BuildError> {
    std::fs::write(path, content)
        .map_err(|e| BuildError::new(format!("Failed to write {}: {}", what, e)))
}

fn set_reload_file_env(cmd_builder: &mut CommandBuilder, ctx: &BuildContext) {
    if let Some(reload_file) = &ctx.reload_file {
        cmd_builder.env(
            "DEVENV_RELOAD_FILE",
            reload_file.to_string_lossy().into_owned(),
        );
    }
}
