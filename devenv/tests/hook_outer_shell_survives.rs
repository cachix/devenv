//! Shell-hook regression tests for bash and fish. Each test asserts one
//! behavior across all available shells; missing shells are skipped.
//!
//! - `outer_shell_survives_cd_out` — #2805: direnv-exported `DEVENV_ROOT`
//!   in the user's outer shell must not turn cd-out into a terminal-killing
//!   `exit`.
//! - `inner_shell_exits_on_cd_out` — hook-spawned shells (marker:
//!   `_DEVENV_HOOK_DIR`) must `exit` and write the exit-dir for the parent.
//! - `no_respawn_inside_devenv_shell` — follow-up to #2815: a manually
//!   entered `devenv shell` (no `_DEVENV_HOOK_DIR`) must not re-enter
//!   activation and spawn a nested shell.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn devenv_bin() -> &'static str {
    env!("CARGO_BIN_EXE_devenv")
}

/// Available shells with the syntax needed to (a) source the hook from the
/// real `devenv` binary and (b) override `PATH`.
fn shells() -> Vec<(&'static str, String, fn(&Path) -> String)> {
    let bash_src = format!(r#"eval "$({} hook bash)""#, devenv_bin());
    let fish_src = format!("{} hook fish | source", devenv_bin());
    [
        ("bash", bash_src, bash_path_override as fn(&Path) -> String),
        ("fish", fish_src, fish_path_override as fn(&Path) -> String),
    ]
    .into_iter()
    .filter(|(s, _, _)| have(s))
    .collect()
}

fn bash_path_override(dir: &Path) -> String {
    format!(r#"export PATH="{}:$PATH""#, dir.display())
}

fn fish_path_override(dir: &Path) -> String {
    format!("set -gx PATH {:?} $PATH", dir)
}

fn have(shell: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {shell}")])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn fake_project() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join(".devenv")).unwrap();
    tmp
}

fn run(shell: &str, script: &str) -> std::process::Output {
    Command::new(shell).arg("-c").arg(script).output().unwrap()
}

#[test]
fn outer_shell_survives_cd_out() {
    for (shell, src, _) in shells() {
        let tmp = fake_project();
        let script = format!(
            "export DEVENV_ROOT={root:?}\n{src}\ncd /\n_devenv_hook\necho SURVIVED\n",
            root = tmp.path(),
        );
        let out = run(shell, &script);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("SURVIVED"),
            "[{shell}] outer shell exited on cd-out (issue #2805).\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

#[test]
fn inner_shell_exits_on_cd_out() {
    for (shell, src, _) in shells() {
        let tmp = fake_project();
        let script = format!(
            "export DEVENV_ROOT={root:?}\nexport _DEVENV_HOOK_DIR={root:?}\n\
             {src}\ncd /\n_devenv_hook\necho SHOULD_NOT_REACH\n",
            root = tmp.path(),
        );
        let out = run(shell, &script);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            !stdout.contains("SHOULD_NOT_REACH"),
            "[{shell}] inner shell did not exit on cd-out.\nstdout: {stdout}",
        );
        let exit_dir = fs::read_to_string(tmp.path().join(".devenv/exit-dir")).unwrap();
        assert_eq!(exit_dir, "/", "[{shell}] exit-dir should record cd target");
    }
}

#[test]
fn no_respawn_inside_devenv_shell() {
    for (shell, src, path_override) in shells() {
        let tmp = fake_project();
        // Shim `devenv` on PATH so any `hook-should-activate` / `shell`
        // invocation from inside the hook is recorded.
        let bin_dir = tempfile::tempdir().unwrap();
        let calls = bin_dir.path().join("calls");
        let fake = bin_dir.path().join("devenv");
        fs::write(
            &fake,
            format!("#!/usr/bin/env bash\necho \"$@\" >> {:?}\nexit 0\n", calls),
        )
        .unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();

        let script = format!(
            "export DEVENV_ROOT={root:?}\ncd {root:?}\n{src}\n{po}\n_devenv_hook\necho DONE\n",
            root = tmp.path(),
            po = path_override(bin_dir.path()),
        );
        let out = run(shell, &script);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("DONE"),
            "[{shell}] hook hung or exited unexpectedly.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr),
        );
        let recorded = fs::read_to_string(&calls).unwrap_or_default();
        assert!(
            recorded.is_empty(),
            "[{shell}] hook re-invoked devenv from inside a manually-entered shell.\n\
             Recorded:\n{recorded}",
        );
    }
}
