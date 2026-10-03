//! Executable launchers for nono's exec-based sandbox.
use devenv_core::DevenvPaths;
use std::path::Path;

fn quote(value: &Path) -> String {
    shell_escape::escape(value.to_string_lossy()).into_owned()
}

pub(crate) fn launcher_script(bash: &str, nono: &Path, paths: &DevenvPaths) -> String {
    // Inherited nono settings must not select a profile or expand these grants.
    let mut script = format!(
        "#!{bash}\nfor name in \"${{!NONO_@}}\"; do unset \"$name\"; done\nexec {} --silent wrap",
        quote(nono)
    );
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
    script.push_str(&format!(" -- {} \"$@\"\n", quote(Path::new(bash))));
    script
}

pub(crate) fn command_script(bash: &str, launcher: &str, command: &str) -> String {
    format!(
        "#!{bash}\nexec {} -c {} -- \"$@\"\n",
        quote(Path::new(launcher)),
        shell_escape::escape(command.into())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
