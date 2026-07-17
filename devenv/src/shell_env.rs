//! Shared shell environment setup for both the reload and no-reload paths.
//!
//! The [`CommandEnv`] trait abstracts over `tokio::process::Command` and
//! `portable_pty::CommandBuilder` so that [`apply_shell_env`] can apply
//! clean/keep filtering and standard env vars to either type.

use devenv_core::config::Clean;

/// Minimal env-manipulation interface shared by `tokio::process::Command`
/// and `portable_pty::CommandBuilder`.
pub(crate) trait CommandEnv {
    fn clear_env(&mut self);
    fn set_env(&mut self, key: &str, value: &str);
}

impl CommandEnv for tokio::process::Command {
    fn clear_env(&mut self) {
        self.env_clear();
    }
    fn set_env(&mut self, key: &str, value: &str) {
        self.env(key, value);
    }
}

impl CommandEnv for devenv_reload::CommandBuilder {
    fn clear_env(&mut self) {
        self.env_clear();
    }
    fn set_env(&mut self, key: &str, value: &str) {
        self.env(key, value);
    }
}

/// Apply clean/keep filtering and standard shell env vars to a command.
///
/// If `clean.enabled`, clears the env and re-sets only the kept vars from
/// the current process.
///
/// Sets `SHELL` only when `shell_path` is `Some` (a successfully resolved
/// binary). `None` means the target shell couldn't be resolved; leaving
/// `SHELL` untouched preserves the caller's real value for `enterShell`
/// (which may itself branch on `$SHELL` and runs before the rcfile's own
/// not-found check), instead of it seeing `SHELL` already overwritten with
/// the name that failed to resolve in the first place.
/// Always sets `DEVENV_CMDLINE`.
pub(crate) fn apply_shell_env(cmd: &mut impl CommandEnv, shell_path: Option<&str>, clean: &Clean) {
    if clean.enabled {
        cmd.clear_env();
        for (k, v) in clean.kept_env_vars() {
            cmd.set_env(&k, &v);
        }
    }

    if let Some(shell_path) = shell_path {
        cmd.set_env("SHELL", shell_path);
    }
    let cmdline = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    cmd.set_env("DEVENV_CMDLINE", &cmdline);
}
