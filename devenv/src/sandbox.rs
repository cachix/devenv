//! Executable launchers for nono's exec-based sandbox.
use devenv_core::{DevenvPaths, config::SandboxConfig};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use std::path::{Path, PathBuf};

fn quote(value: &Path) -> String {
    shell_escape::escape(value.to_string_lossy()).into_owned()
}

/// Resolve grants before launching nono, keeping path expansion independent of shell syntax.
pub(crate) fn resolve_paths(
    paths: &SandboxConfig,
    base: &Path,
    home: Option<&Path>,
) -> Result<SandboxConfig> {
    let resolve = |path: &PathBuf| -> Result<PathBuf> {
        if path.as_os_str().is_empty() {
            bail!("sandbox.read and sandbox.write entries must not be empty");
        }
        let expanded = if let Ok(rest) = path.strip_prefix("~") {
            home.ok_or_else(|| {
                miette::miette!(
                    "Cannot expand sandbox path {}: HOME is not set",
                    path.display()
                )
            })?
            .join(rest)
        } else {
            path.clone()
        };
        let resolved = if expanded.is_absolute() {
            expanded
        } else {
            base.join(expanded)
        };
        let canonical = resolved
            .canonicalize()
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "Invalid sandbox path {}: configured files and directories must exist",
                    resolved.display()
                )
            })?;
        if !canonical.is_file() && !canonical.is_dir() {
            bail!(
                "Invalid sandbox path {}: expected a file or directory",
                canonical.display()
            );
        }
        Ok(canonical)
    };
    Ok(SandboxConfig {
        read: paths.read.iter().map(&resolve).collect::<Result<_>>()?,
        write: paths.write.iter().map(resolve).collect::<Result<_>>()?,
        ..paths.clone()
    })
}

pub(crate) fn launcher_script(
    bash: &str,
    nono: &Path,
    paths: &DevenvPaths,
    extra: &SandboxConfig,
) -> String {
    // -p suppresses BASH_ENV and imported functions before nono applies the sandbox.
    // The bash executed inside the sandbox starts normally.
    // Inherited nono settings must not select a profile or expand these grants.
    let mut script = format!(
        "#!{bash} -p\nfor name in \"${{!NONO_@}}\"; do unset \"$name\"; done\nexec {} --silent wrap",
        quote(nono)
    );
    if !extra.networking.is_enabled() {
        script.push_str(" --block-net");
    }
    // These are runtime grants, not permission to modify the Nix store.
    for path in [
        Some(&paths.root),
        Some(&paths.dotfile),
        paths.state.as_ref(),
        Some(&paths.runtime),
        Some(&paths.tmp),
    ]
    .into_iter()
    .flatten()
    {
        script.push_str(&format!(" --allow {}", quote(path)));
    }
    for path in [
        "/nix/store",
        "/etc",
        "/usr",
        "/bin",
        "/sbin",
        "/lib",
        "/lib64",
        "/System/Library",
        "/System/Applications",
        "/Library",
    ] {
        if Path::new(path).exists() {
            script.push_str(&format!(" --read {}", quote(Path::new(path))));
        }
    }
    for (paths, directory_flag, file_flag) in [
        (&extra.read, "--read", "--read-file"),
        (&extra.write, "--allow", "--allow-file"),
    ] {
        for path in paths {
            let flag = if path.is_dir() {
                directory_flag
            } else {
                file_flag
            };
            script.push_str(&format!(" {flag} {}", quote(path)));
        }
    }
    script.push_str(&format!(" -- {} \"$@\"\n", quote(Path::new(bash))));
    script
}

pub(crate) fn command_script(bash: &str, launcher: &str, command: &str) -> String {
    format!(
        "#!{bash} -p\nexec {} -c {} -- \"$@\"\n",
        quote(Path::new(launcher)),
        shell_escape::escape(command.into())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launchers_ignore_startup_code_and_imported_functions_before_nono() {
        use std::os::unix::fs::PermissionsExt;
        let bash = std::process::Command::new("bash")
            .args(["-p", "-c", "printf '%s' \"$BASH\""])
            .output()
            .unwrap();
        assert!(bash.status.success());
        let bash = String::from_utf8(bash.stdout).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let startup = dir.path().join("startup");
        std::fs::write(&startup, "exit 91\n").unwrap();
        let nono = dir.path().join("nono");
        std::fs::write(
            &nono,
            format!("#!{bash} -p\nprintf 'nono %s\\n' \"${{NONO_PROFILE-unset}}\"\n"),
        )
        .unwrap();
        let paths = DevenvPaths {
            root: dir.path().into(),
            lock_file: dir.path().join("devenv.lock"),
            dotfile: dir.path().into(),
            dot_gc: dir.path().into(),
            home_gc: dir.path().into(),
            tmp: dir.path().into(),
            runtime: dir.path().into(),
            state: None,
            git_root: None,
        };
        let launcher = dir.path().join("launcher");
        std::fs::write(
            &launcher,
            launcher_script(&bash, &nono, &paths, &SandboxConfig::default()),
        )
        .unwrap();
        let command = dir.path().join("command");
        std::fs::write(
            &command,
            command_script(&bash, launcher.to_str().unwrap(), "exit 23"),
        )
        .unwrap();
        for path in [&nono, &launcher, &command] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        for path in [&launcher, &command] {
            let result = std::process::Command::new(path)
                .env("BASH_ENV", &startup)
                .env("BASH_FUNC_exec%%", "() { exit 92; }")
                .env("BASH_FUNC_unset%%", "() { exit 93; }")
                .env("SHELLOPTS", "noexec")
                .env("NONO_PROFILE", "unexpected")
                .output()
                .unwrap();
            assert!(result.status.success(), "{}: {result:?}", path.display());
            assert_eq!(result.stdout, b"nono unset\n");
        }
    }

    #[test]
    fn sandbox_paths_resolve_relative_home_and_symlink_paths() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(home.join("cache")).unwrap();
        std::fs::write(home.join("file with spaces"), "data").unwrap();
        symlink(home.join("file with spaces"), project.join("linked-file")).unwrap();
        let configured = SandboxConfig {
            read: vec!["linked-file".into()],
            write: vec!["~/cache".into()],
            ..SandboxConfig::default()
        };
        let resolved = resolve_paths(&configured, &project, Some(&home)).unwrap();
        assert_eq!(
            resolved.read,
            vec![home.join("file with spaces").canonicalize().unwrap()]
        );
        assert_eq!(
            resolved.write,
            vec![home.join("cache").canonicalize().unwrap()]
        );
        assert!(resolve_paths(&configured, &project, None).is_err());
        assert!(
            resolve_paths(
                &SandboxConfig {
                    read: vec!["missing".into()],
                    write: vec![],
                    ..SandboxConfig::default()
                },
                &project,
                Some(&home)
            )
            .is_err()
        );
        assert!(
            resolve_paths(
                &SandboxConfig {
                    read: vec!["".into()],
                    write: vec![],
                    ..SandboxConfig::default()
                },
                &project,
                Some(&home)
            )
            .is_err()
        );
    }

    #[test]
    fn command_launcher_preserves_shell_source_and_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let launcher = dir.path().join("launcher with spaces");
        std::fs::write(&launcher, "#!/bin/sh\nexec /bin/sh \"$@\"\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755)).unwrap();
        let script = command_script(
            "/bin/sh",
            launcher.to_str().unwrap(),
            "printf '%s\\n' \"$1\"; exit 23",
        );
        let result = std::process::Command::new("/bin/sh")
            .args(["-c", &script, "--", "literal $(false) ' text"])
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(23));
        assert_eq!(
            String::from_utf8(result.stdout).unwrap(),
            "literal $(false) ' text\n"
        );
    }
}
