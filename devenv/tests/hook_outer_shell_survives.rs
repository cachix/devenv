//! Shell-hook regression tests across bash, zsh, fish, and nu. Each test
//! asserts one behavior; missing shells are skipped.
//!
//! - `outer_shell_survives_cd_out` — #2805
//! - `inner_shell_exits_on_cd_out` — hook-spawned shell must `exit` + write exit-dir
//! - `no_respawn_inside_devenv_shell` — follow-up to #2815

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn devenv_bin() -> &'static str {
    env!("CARGO_BIN_EXE_devenv")
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

/// A shimmed `devenv` on PATH that records its argv to a file.
fn devenv_shim() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let calls = dir.path().join("calls");
    let bin = dir.path().join("devenv");
    fs::write(&bin, format!("#!/bin/sh\necho \"$@\" >> {:?}\n", calls)).unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    (dir, calls)
}

/// bash, zsh, fish — sufficiently similar that one template covers all three.
fn shells() -> Vec<(&'static str, String, fn(&Path) -> String)> {
    let bin = devenv_bin();
    [
        (
            "bash",
            format!(r#"eval "$({bin} hook bash)""#),
            posix_path_override as fn(&Path) -> String,
        ),
        (
            "zsh",
            format!(r#"eval "$({bin} hook zsh)""#),
            posix_path_override as fn(&Path) -> String,
        ),
        (
            "fish",
            format!("{bin} hook fish | source"),
            fish_path_override as fn(&Path) -> String,
        ),
    ]
    .into_iter()
    .filter(|(s, _, _)| have(s))
    .collect()
}

fn posix_path_override(dir: &Path) -> String {
    format!(r#"export PATH="{}:$PATH""#, dir.display())
}

fn fish_path_override(dir: &Path) -> String {
    format!("set -gx PATH {:?} $PATH", dir)
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
        let (_bin_dir, calls) = devenv_shim();
        let script = format!(
            "export DEVENV_ROOT={root:?}\ncd {root:?}\n{src}\n{po}\n_devenv_hook\necho DONE\n",
            root = tmp.path(),
            po = path_override(calls.parent().unwrap()),
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

// Nu's env_change.PWD hook only fires in interactive sessions, so nu tests
// source the hook (which defines `_devenv_hook`) and call it directly.
// Different enough syntactically from the posix shells that folding it into
// the loop above forces more abstraction than it saves.

fn run_nu(setup: &str, body: &str) -> std::process::Output {
    let hook_dir = tempfile::tempdir().unwrap();
    let hook_path = hook_dir.path().join("hook.nu");
    let hook_gen = Command::new(devenv_bin())
        .args(["hook", "nu"])
        .output()
        .unwrap();
    assert!(hook_gen.status.success(), "devenv hook nu failed");
    fs::write(&hook_path, &hook_gen.stdout).unwrap();
    let script = format!(
        "{setup}\nsource {hook:?}\ncd {root:?}\n{body}\n",
        hook = hook_path,
        root = hook_dir.path(),
    );
    Command::new("nu").arg("-c").arg(&script).output().unwrap()
}

#[test]
fn nu_outer_shell_survives_cd_out() {
    if !have("nu") {
        return;
    }
    let tmp = fake_project();
    let out = run_nu(
        &format!(r#"$env.DEVENV_ROOT = "{}""#, tmp.path().display()),
        "cd /; _devenv_hook; print SURVIVED",
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("SURVIVED"),
        "[nu] outer shell exited on cd-out (issue #2805).\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn nu_inner_shell_exits_on_cd_out() {
    if !have("nu") {
        return;
    }
    let tmp = fake_project();
    let out = run_nu(
        &format!(
            r#"$env.DEVENV_ROOT = "{root}"; $env._DEVENV_HOOK_DIR = "{root}""#,
            root = tmp.path().display(),
        ),
        "cd /; _devenv_hook; print SHOULD_NOT_REACH",
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("SHOULD_NOT_REACH"),
        "[nu] inner shell did not exit on cd-out.\nstdout: {stdout}",
    );
    let exit_dir = fs::read_to_string(tmp.path().join(".devenv/exit-dir")).unwrap();
    assert_eq!(exit_dir, "/", "[nu] exit-dir should record cd target");
}

#[test]
fn nu_no_respawn_inside_devenv_shell() {
    if !have("nu") {
        return;
    }
    let tmp = fake_project();
    let (_bin_dir, calls) = devenv_shim();
    let setup = format!(
        r#"$env.DEVENV_ROOT = "{root}"; $env.PATH = ($env.PATH | prepend "{shim}")"#,
        root = tmp.path().display(),
        shim = calls.parent().unwrap().display(),
    );
    let body = format!(
        r#"cd {root:?}; _devenv_hook; print DONE"#,
        root = tmp.path(),
    );
    let out = run_nu(&setup, &body);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("DONE"),
        "[nu] hook hung or exited unexpectedly.\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    let recorded = fs::read_to_string(&calls).unwrap_or_default();
    assert!(
        recorded.is_empty(),
        "[nu] hook re-invoked devenv from inside a manually-entered shell.\n\
         Recorded:\n{recorded}",
    );
}
