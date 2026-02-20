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
/// Always sets `SHELL` and `DEVENV_CMDLINE`.
pub(crate) fn apply_shell_env(cmd: &mut impl CommandEnv, bash_path: &str, clean: &Clean) {
    if clean.enabled {
        cmd.clear_env();
        for (k, v) in std::env::vars() {
            if clean.keep.contains(&k) {
                cmd.set_env(&k, &v);
            }
        }
    }

    cmd.set_env("SHELL", bash_path);
    let cmdline = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    cmd.set_env("DEVENV_CMDLINE", &cmdline);
}
