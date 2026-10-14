//! Shared shell environment setup for both the reload and no-reload paths.
//!
//! The [`CommandEnv`] trait abstracts over `tokio::process::Command` and
//! `portable_pty::CommandBuilder` so that [`apply_shell_env`] can apply
//! clean/keep filtering and standard env vars to either type.

use devenv_core::config::Clean;
use std::path::Path;

const SHELL_CWD_VAR: &str = "_DEVENV_SHELL_CWD";

pub(crate) const CD_INTO_SHELL_CWD: &str = r#"if [ -n "${_DEVENV_SHELL_CWD:-}" ]; then
    _devenv_oldpwd=${OLDPWD-}
    builtin cd -- "$_DEVENV_SHELL_CWD"
    if [ -n "$_devenv_oldpwd" ]; then OLDPWD=$_devenv_oldpwd; else unset OLDPWD; fi
    unset _devenv_oldpwd
fi
unset _DEVENV_SHELL_CWD
"#;

/// Minimal env-manipulation interface shared by `tokio::process::Command`
/// and `portable_pty::CommandBuilder`.
pub(crate) trait CommandEnv {
    fn clear_env(&mut self);
    fn set_env(&mut self, key: &str, value: &str);
    fn remove_env(&mut self, key: &str);
}

impl CommandEnv for tokio::process::Command {
    fn clear_env(&mut self) {
        self.env_clear();
    }
    fn set_env(&mut self, key: &str, value: &str) {
        self.env(key, value);
    }
    fn remove_env(&mut self, key: &str) {
        self.env_remove(key);
    }
}

impl CommandEnv for devenv_reload::CommandBuilder {
    fn clear_env(&mut self) {
        self.env_clear();
    }
    fn set_env(&mut self, key: &str, value: &str) {
        self.env(key, value);
    }
    fn remove_env(&mut self, key: &str) {
        self.env_remove(key);
    }
}

/// Apply clean/keep filtering and standard shell env vars to a command.
///
/// If `clean.enabled`, clears the env and re-sets only the kept vars from
/// the current process.
/// Always sets `SHELL` and `DEVENV_CMDLINE`, and tells [`CD_INTO_SHELL_CWD`]
/// where to move once the environment is set up.
pub(crate) fn apply_shell_env(
    cmd: &mut impl CommandEnv,
    shell_path: &str,
    clean: &Clean,
    shell_cwd: Option<&Path>,
) {
    if clean.enabled {
        cmd.clear_env();
        for (k, v) in clean.kept_env_vars() {
            cmd.set_env(&k, &v);
        }
    }

    cmd.set_env("SHELL", shell_path);
    let cmdline = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    cmd.set_env("DEVENV_CMDLINE", &cmdline);
    match shell_cwd {
        Some(dir) => cmd.set_env(SHELL_CWD_VAR, &dir.to_string_lossy()),
        None => cmd.remove_env(SHELL_CWD_VAR),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn run_cd_into_shell_cwd(shell_cwd: Option<&Path>, oldpwd: Option<&str>) -> String {
        let mut cmd = Command::new("bash");
        cmd.current_dir("/")
            .arg("-c")
            .arg(format!(
                "cd() {{ echo overridden; }}\n{CD_INTO_SHELL_CWD}\
                 printf '%s|%s|%s' \"$PWD\" \"${{OLDPWD-unset}}\" \"${{{SHELL_CWD_VAR}-unset}}\""
            ))
            .env_remove("OLDPWD")
            .env_remove(SHELL_CWD_VAR);
        if let Some(dir) = shell_cwd {
            cmd.env(SHELL_CWD_VAR, dir);
        }
        if let Some(oldpwd) = oldpwd {
            cmd.env("OLDPWD", oldpwd);
        }
        let output = cmd.output().unwrap();
        String::from_utf8(output.stdout).unwrap()
    }

    #[test]
    fn cd_into_shell_cwd_moves_into_the_invoking_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(tmp.path())
            .unwrap()
            .join("it's a \"dir\" $HOME");
        std::fs::create_dir(&dir).unwrap();
        let dir_str = dir.to_string_lossy();

        assert_eq!(
            run_cd_into_shell_cwd(Some(&dir), None),
            format!("{dir_str}|unset|unset")
        );
        assert_eq!(
            run_cd_into_shell_cwd(Some(&dir), Some("/tmp")),
            format!("{dir_str}|/tmp|unset")
        );
        assert_eq!(run_cd_into_shell_cwd(None, Some("/tmp")), "/|/tmp|unset");
    }
}
