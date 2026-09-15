use crate::tui::{UserConfig, UserConfigError};
use miette::{Result, miette};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

pub fn path(override_path: Option<&Path>) -> Result<PathBuf> {
    match override_path {
        Some(path) => Ok(path.to_path_buf()),
        None => devenv_core::paths::resolve_user_config_file().ok_or_else(|| {
            miette!("could not resolve the user configuration directory for devenv")
        }),
    }
}

pub fn load(override_path: Option<&Path>) -> Result<UserConfig> {
    let explicit = override_path.is_some();
    let path = path(override_path)?;
    match UserConfig::load(&path) {
        Ok(config) => Ok(config),
        Err(UserConfigError::Read { source, .. })
            if !explicit && source.kind() == ErrorKind::NotFound =>
        {
            Ok(UserConfig::default())
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_file_is_loaded() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.yaml");
        std::fs::write(
            &path,
            "version: 1\ntui:\n  behavior:\n    log_preview_lines: 23\nshell:\n  keybindings:\n    toggle_pause: [f12]\n",
        )
        .unwrap();
        let config = load(Some(&path)).unwrap();
        assert_eq!(config.tui.behavior.log_preview_lines, 23);
        assert_eq!(
            config
                .shell
                .resolve()
                .unwrap()
                .key_label(devenv_shell::keybindings::ShellAction::TogglePause, false),
            Some("F12".to_string())
        );
    }

    #[test]
    fn explicit_missing_file_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let error = load(Some(&directory.path().join("missing.yaml"))).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("failed to read user configuration")
        );
    }
}
