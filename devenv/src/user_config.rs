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

/// Combine personal defaults and project settings for every CLI invocation.
pub fn sandbox(
    project: &devenv_core::config::SandboxConfig,
    override_path: Option<&Path>,
    project_root: &Path,
) -> Result<devenv_core::config::SandboxConfig> {
    use devenv_core::config::{SandboxConfig, SandboxNetworking};
    // An explicit project disable does not load or validate personal sandbox settings.
    if project.enable == Some(false) {
        return Ok(project.clone());
    }
    let user = load(override_path)?.sandbox;
    let mut resolved = project.clone();
    resolved.enable = project.enable.or(user.enable);
    resolved.networking.enable = project.networking.enable.or(user.networking.enable);
    if !resolved.is_enabled() {
        return Ok(resolved);
    }
    let user_path = path(override_path)?;
    let user_paths = SandboxConfig {
        read: user.read,
        write: user.write,
        networking: SandboxNetworking::default(),
        ..SandboxConfig::default()
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut paths = crate::sandbox::resolve_paths(&resolved, project_root, home.as_deref())?;
    let global = crate::sandbox::resolve_paths(
        &user_paths,
        user_path.parent().unwrap_or(Path::new(".")),
        home.as_deref(),
    )?;
    paths.read.extend(global.read);
    paths.write.extend(global.write);
    paths.read.sort();
    paths.read.dedup();
    paths.write.sort();
    paths.write.dedup();
    // A read/write grant supersedes a read-only grant for the same resolved path.
    paths.read.retain(|path| !paths.write.contains(path));
    Ok(paths)
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
    fn sandbox_merges_user_and_project_settings_from_their_own_roots() {
        use devenv_core::config::SandboxConfig;
        let directory = tempfile::tempdir().unwrap();
        let project_root = directory.path().join("project");
        let config_root = directory.path().join("user");
        std::fs::create_dir_all(project_root.join("shared")).unwrap();
        std::fs::create_dir_all(config_root.join("personal")).unwrap();
        let config_path = config_root.join("config.yaml");
        let shared = project_root.join("shared").canonicalize().unwrap();
        std::fs::write(
            &config_path,
            format!(
                "version: 1\nsandbox:\n  enable: true\n  read: [personal]\n  write: [{}]\n  networking:\n    enable: false\n",
                shared.display()
            ),
        )
        .unwrap();
        let project = SandboxConfig {
            read: vec!["shared".into()],
            write: vec![],
            ..SandboxConfig::default()
        };
        let merged = sandbox(&project, Some(&config_path), &project_root).unwrap();
        assert_eq!(
            merged.read,
            vec![config_root.join("personal").canonicalize().unwrap()]
        );
        assert_eq!(merged.write, vec![shared]);
        assert!(merged.is_enabled());
        assert!(!merged.networking.is_enabled());
        let mut enabled_network = project.clone();
        enabled_network.networking.enable = Some(true);
        assert!(
            sandbox(&enabled_network, Some(&config_path), &project_root)
                .unwrap()
                .networking
                .is_enabled()
        );
        // Explicit project disable wins over the global enable and skips path validation.
        let disabled = SandboxConfig {
            enable: Some(false),
            read: vec!["missing".into()],
            ..SandboxConfig::default()
        };
        assert!(
            !sandbox(&disabled, Some(&config_path), &project_root)
                .unwrap()
                .is_enabled()
        );
        assert!(
            !sandbox(
                &disabled,
                Some(&directory.path().join("missing.yaml")),
                &project_root
            )
            .unwrap()
            .is_enabled()
        );
    }

    #[test]
    fn user_sandbox_rejects_unknown_permissions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.yaml");
        std::fs::write(&path, "version: 1\nsandbox:\n  write_only: [cache]\n").unwrap();
        assert!(load(Some(&path)).is_err());
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
