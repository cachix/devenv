//! Shell dialect abstraction for supporting different shell types (bash, zsh, fish, etc.).
//!
//! The [`ShellDialect`] trait encapsulates shell-specific behavior for interactive
//! shell sessions, including rcfile generation, environment diff tracking, reload
//! hooks, and launch arguments.

mod bash;
mod fish;
mod nushell;
mod zsh;

pub use bash::BashDialect;
pub use fish::FishDialect;
pub use nushell::NushellDialect;
pub use zsh::ZshDialect;

use std::path::{Path, PathBuf};

/// Shell-specific behavior for interactive sessions.
pub trait ShellDialect: Send + Sync {
    /// Shell name for display/logging (e.g., "bash", "zsh", "fish").
    fn name(&self) -> &str;

    /// Arguments to launch an interactive shell with a custom init script.
    /// Returns (prefix_args, suffix_args) that go around the rcfile path.
    /// e.g. bash: (["--noprofile", "--rcfile"], ["-i"])
    fn interactive_args(&self) -> InteractiveArgs;

    /// Generate the rcfile/init script content for an interactive shell.
    fn rcfile_content(&self, ctx: &RcfileContext) -> String;

    /// Generate environment diff helper functions (for hot-reload tracking).
    fn env_diff_helpers(&self) -> &str;

    /// Generate the hot-reload hook script (keybinding + prompt hook).
    fn reload_hook(&self, reload_file: &Path) -> String;

    /// Path to the user's shell rc file (e.g., ~/.bashrc, ~/.zshrc).
    fn user_rcfile(&self) -> Option<PathBuf>;

    /// Generate a shell-specific PS1/prompt prefix for "(devenv)".
    fn prompt_prefix(&self) -> &str;

    /// Write supplementary init files (e.g., zsh's ZDOTDIR .zshrc).
    /// Default implementation is a no-op (bash doesn't need extra files).
    fn write_init_files(&self, ctx: &RcfileContext) -> std::io::Result<()> {
        let _ = ctx;
        Ok(())
    }
}

/// Arguments for launching an interactive shell with a custom init script.
pub struct InteractiveArgs {
    /// Args before the rcfile path (e.g., `["--noprofile", "--rcfile"]` for bash).
    pub prefix: Vec<String>,
    /// Args after the rcfile path (e.g., `["-i"]` for bash).
    pub suffix: Vec<String>,
}

/// Look up a dialect by name, defaulting to bash if no match.
pub fn create_dialect(shell_name: &str) -> Box<dyn ShellDialect> {
    match shell_name {
        "zsh" => Box::new(ZshDialect),
        "fish" => Box::new(FishDialect),
        "nu" => Box::new(NushellDialect),
        _ => Box::new(BashDialect),
    }
}

/// Context passed to [`ShellDialect::rcfile_content`] for generating the init script.
pub struct RcfileContext<'a> {
    /// Path to the devenv environment script to source.
    pub env_script_path: &'a Path,
    /// Environment diff helper functions.
    pub env_diff_helpers: &'a str,
    /// Reload hook script (empty if no reload).
    pub reload_hook: &'a str,
    /// Path to the target shell binary (e.g., /usr/bin/zsh). None for bash (no exec needed).
    pub target_shell_path: Option<&'a str>,
    /// Directory for writing shell init files (e.g., .devenv/).
    pub init_dir: &'a Path,
}
