//! Shell-hook regression tests across bash, zsh, fish, and nu. Each test
//! asserts one behavior; missing shells are skipped.
//!
//! - `outer_shell_survives_cd_out` — #2805
//! - `inner_shell_exits_on_cd_out` — hook-spawned shell must `exit` + write exit-dir
//! - `hook_dir_marker_does_not_leak_to_child_shell` — #2861
//! - `no_respawn_inside_devenv_shell` — follow-up to #2815
//! - `fish_deferred_activation_skips_if_already_active` — direnv/devenv double-activation race
//! - `posix_activates_sibling_after_cd_out` — #2944

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

fn posix_shells() -> Vec<(&'static str, String)> {
    let bin = devenv_bin();
    [
        ("bash", format!(r#"eval "$({bin} hook bash)""#)),
        ("zsh", format!(r#"eval "$({bin} hook zsh)""#)),
    ]
    .into_iter()
    .filter(|(s, _)| have(s))
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

fn sibling_activation_shim(project_a: &Path, project_b: &Path) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let calls = dir.path().join("calls");
    let bin = dir.path().join("devenv");
    fs::write(
        &bin,
        format!(
            r#"#!/bin/sh
set -eu
case "$1" in
  hook-should-activate)
    if [ -d "$PWD/.devenv" ]; then
      printf '%s\n' "$PWD"
    fi
    ;;
  shell)
    printf 'shell %s\n' "$PWD" >> {calls:?}
    if [ "$PWD" = {project_a:?} ]; then
      printf '%s' {project_b:?} > {project_a:?}/.devenv/exit-dir
    fi
    ;;
  *)
    printf '%s\n' "$*" >> {calls:?}
    ;;
esac
"#,
            calls = calls,
            project_a = project_a.display().to_string(),
            project_b = project_b.display().to_string(),
        ),
    )
    .unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    (dir, calls)
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
fn hook_dir_marker_does_not_leak_to_child_shell() {
    // A new shell started from inside an active devenv shell (a new
    // tmux/zellij pane, a manually started nested shell, ...) inherits
    // `DEVENV_ROOT` and `_DEVENV_HOOK_DIR` via the process environment. If it
    // also re-sources the hook (as any normal interactive rc file would), it
    // must not conclude it is itself hook-spawned and `exit` on cd-out —
    // nothing set up a parent to catch that exit, so doing so would just
    // kill the pane/session (issue #2861).
    for (shell, src, _) in shells() {
        let tmp = fake_project();
        let child_script = format!("{src}\ncd /\n_devenv_hook\necho SURVIVED\n");
        let script = format!(
            "export DEVENV_ROOT={root:?}\nexport _DEVENV_HOOK_DIR={root:?}\n\
             {src}\n{shell} -c '{child_script}'\n",
            root = tmp.path(),
        );
        let out = run(shell, &script);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("SURVIVED"),
            "[{shell}] a shell spawned from inside an active devenv shell inherited \
             _DEVENV_HOOK_DIR and exited on cd-out.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr),
        );
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

#[test]
fn fish_deferred_activation_skips_if_already_active() {
    // Fish defers activation to the next prompt (see the comment on
    // `_devenv_hook` in hook.fish) to avoid spawning inside a PWD event
    // handler. In between the initial decision and that deferred prompt,
    // something else (direnv loading a `.envrc` with `use devenv`, a
    // manually entered devenv shell, ...) may have already activated an
    // environment for this directory. `_devenv_hook_activate` must notice
    // `DEVENV_ROOT` is now set and skip, rather than stacking a redundant
    // devenv shell on top.
    if !have("fish") {
        return;
    }
    let tmp = fake_project();
    let dir = tempfile::tempdir().unwrap();
    let calls = dir.path().join("calls");
    let shim_bin = dir.path().join("devenv");
    fs::write(
        &shim_bin,
        format!(
            r#"#!/bin/sh
case "$1" in
  hook-should-activate)
    printf '%s\n' {root:?}
    ;;
  shell)
    printf 'shell %s\n' "$PWD" >> {calls:?}
    ;;
esac
"#,
            root = tmp.path(),
        ),
    )
    .unwrap();
    fs::set_permissions(&shim_bin, fs::Permissions::from_mode(0o755)).unwrap();

    let bin = devenv_bin();
    let script = format!(
        // Explicitly erase: a `devenv shell` invoked to run this very test
        // suite would otherwise leak `DEVENV_ROOT` into the spawned fish,
        // masking the "not yet activated" starting state this test needs.
        "set -e DEVENV_ROOT; set -e _DEVENV_HOOK_DIR\n\
         {bin} hook fish | source\ncd {root:?}\n\
         {po}\n\
         _devenv_hook\n\
         set -gx DEVENV_ROOT {root:?}\n\
         _devenv_hook_prompt\n\
         echo DONE\n",
        po = fish_path_override(dir.path()),
        root = tmp.path(),
    );
    let out = run("fish", &script);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("DONE"),
        "fish hook hung or exited unexpectedly.\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    let recorded = fs::read_to_string(&calls).unwrap_or_default();
    assert!(
        recorded.is_empty(),
        "fish spawned a redundant devenv shell after DEVENV_ROOT was set by \
         something else (e.g. direnv) between the cd and the deferred prompt.\n\
         Recorded:\n{recorded}",
    );
}

#[test]
fn posix_activates_sibling_after_cd_out() {
    for (shell, src) in posix_shells() {
        let parent = tempfile::tempdir().unwrap();
        let project_a = parent.path().join("project-a");
        let project_b = parent.path().join("project-b");
        fs::create_dir_all(project_a.join(".devenv")).unwrap();
        fs::create_dir_all(project_b.join(".devenv")).unwrap();
        let (_bin_dir, calls) = sibling_activation_shim(&project_a, &project_b);
        let script = format!(
            "unset DEVENV_ROOT _DEVENV_HOOK_DIR\n\
             {src}\n\
             {po}\n\
             cd {project_a:?}\n\
             _devenv_hook\n\
             _devenv_hook\n\
             printf 'PWD=%s\\n' \"$PWD\"\n",
            po = posix_path_override(calls.parent().unwrap()),
        );
        let out = run(shell, &script);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "[{shell}] sibling activation script failed.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr),
        );
        assert!(
            stdout.contains(&format!("PWD={}", project_b.display())),
            "[{shell}] parent shell did not follow exit-dir to sibling.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr),
        );
        let recorded = fs::read_to_string(&calls).unwrap_or_default();
        assert!(
            recorded.contains(&format!("shell {}", project_b.display())),
            "[{shell}] sibling project was not activated after cd-out.\nRecorded:\n{recorded}\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr),
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
fn nu_hook_dir_marker_does_not_leak_to_child_shell() {
    if !have("nu") {
        return;
    }
    let tmp = fake_project();
    let hook_path = tmp.path().join("hook.nu");
    let hook_gen = Command::new(devenv_bin())
        .args(["hook", "nu"])
        .output()
        .unwrap();
    assert!(hook_gen.status.success(), "devenv hook nu failed");
    fs::write(&hook_path, &hook_gen.stdout).unwrap();

    let root = tmp.path();
    let child_script = format!("source {hook_path:?}\ncd /\n_devenv_hook\nprint SURVIVED\n");
    let script = format!(
        "$env.DEVENV_ROOT = \"{root}\"\n$env._DEVENV_HOOK_DIR = \"{root}\"\n\
         source {hook_path:?}\ncd {root:?}\n^nu -c '{child_script}'\n",
        root = root.display(),
    );
    let out = Command::new("nu").arg("-c").arg(&script).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("SURVIVED"),
        "[nu] a shell spawned from inside an active devenv shell inherited \
         _DEVENV_HOOK_DIR and exited on cd-out.\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
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
