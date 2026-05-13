//! Regression: shell-integration hooks must not call `exit` in the user's
//! outer shell. Direnv (and similar tools) may export `DEVENV_ROOT` into the
//! outer shell's env, and an unguarded `exit` on cd-out closes the terminal.
//! See https://github.com/cachix/devenv/issues/2805

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn devenv_bin() -> &'static str {
    env!("CARGO_BIN_EXE_devenv")
}

fn fake_project() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join(".devenv")).unwrap();
    tmp
}

fn run(shell: &str, script: &str) -> Output {
    Command::new(shell).arg("-c").arg(script).output().unwrap()
}

fn skip_if_missing(shell: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {shell}")])
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
}

/// Outer shell: `DEVENV_ROOT` set but `_DEVENV_HOOK_DIR` unset. cd-out must
/// NOT terminate the shell.
fn outer_shell_survives(shell: &str, hook_name: &str, source_cmd: &str, root: &Path) {
    let script = format!(
        r#"
        export DEVENV_ROOT={root:?}
        {source_cmd}
        cd /
        _devenv_hook
        echo SURVIVED
        "#,
        root = root,
        source_cmd = source_cmd,
    );
    let out = run(shell, &script);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("SURVIVED"),
        "{shell} outer shell exited on cd-out via `devenv hook {hook_name}` (issue #2805).\n\
         stdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Inner (hook-spawned) shell: both `_DEVENV_HOOK_DIR` and `DEVENV_ROOT`
/// set. cd-out MUST exit the shell and write the exit-dir for the parent.
fn inner_shell_exits(shell: &str, hook_name: &str, source_cmd: &str, root: &Path) {
    let script = format!(
        r#"
        export DEVENV_ROOT={root:?}
        export _DEVENV_HOOK_DIR={root:?}
        {source_cmd}
        cd /
        _devenv_hook
        echo SHOULD_NOT_REACH
        "#,
        root = root,
        source_cmd = source_cmd,
    );
    let out = run(shell, &script);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("SHOULD_NOT_REACH"),
        "{shell} inner shell did not exit on cd-out via `devenv hook {hook_name}`.\n\
         stdout: {stdout}",
    );
    let exit_dir = fs::read_to_string(root.join(".devenv/exit-dir")).unwrap();
    assert_eq!(exit_dir, "/", "{shell}: exit-dir should record cd target");
}

#[test]
fn bash_outer_shell_survives_cd_out_when_only_devenv_root_set() {
    let tmp = fake_project();
    let src = format!(r#"eval "$({} hook bash)""#, devenv_bin());
    outer_shell_survives("bash", "bash", &src, tmp.path());
}

#[test]
fn bash_inner_shell_exits_on_cd_out_and_writes_exit_dir() {
    let tmp = fake_project();
    let src = format!(r#"eval "$({} hook bash)""#, devenv_bin());
    inner_shell_exits("bash", "bash", &src, tmp.path());
}

#[test]
fn fish_outer_shell_survives_cd_out_when_only_devenv_root_set() {
    if skip_if_missing("fish") {
        eprintln!("fish not in PATH; skipping");
        return;
    }
    let tmp = fake_project();
    // `_devenv_hook` is fish's PWD-change handler; calling it directly here
    // mirrors what fish does on `cd` in an interactive session.
    let src = format!("{} hook fish | source", devenv_bin());
    outer_shell_survives("fish", "fish", &src, tmp.path());
}

#[test]
fn fish_inner_shell_exits_on_cd_out_and_writes_exit_dir() {
    if skip_if_missing("fish") {
        eprintln!("fish not in PATH; skipping");
        return;
    }
    let tmp = fake_project();
    let src = format!("{} hook fish | source", devenv_bin());
    inner_shell_exits("fish", "fish", &src, tmp.path());
}
