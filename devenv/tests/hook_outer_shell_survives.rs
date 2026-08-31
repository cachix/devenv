//! Shell-hook regression tests across bash, zsh, fish, and nu. Each test
//! asserts one behavior; missing shells are skipped.
//!
//! - `outer_shell_survives_cd_out` — #2805
//! - `inner_shell_exits_on_cd_out` — hook-spawned shell must `exit` + write exit-dir
//! - `hook_dir_marker_does_not_leak_to_child_shell` — #2861
//! - `no_respawn_inside_devenv_shell` — follow-up to #2815
//! - `fish_activation_skips_if_already_active` — avoid stacking devenv shells
//! - `fish_follow_cd_out_preserves_history_for_cd_dash` — #2853
//! - `posix_activates_sibling_after_cd_out` — #2944
//! - `activates_again_after_returning_to_the_same_project` — stale
//!   `_DEVENV_HOOK_ACTIVATED` after a follow-cd (`nu_` variant for nushell)

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
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

/// A shimmed `devenv` that makes the hook activate `project` and records the
/// spawned `devenv shell` argv with NUL separators, preserving exact argument
/// boundaries (including empty arguments and embedded newlines).
fn argv_recording_devenv_shim(project: &Path) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let calls = dir.path().join("calls");
    let bin = dir.path().join("devenv");
    fs::write(
        &bin,
        format!(
            r#"#!/bin/sh
case "$1" in
  hook-should-activate)
    printf '%s\n' {project:?}
    ;;
  shell)
    printf '%s\0' "$@" > {calls:?}
    ;;
esac
"#,
        ),
    )
    .unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    (dir, calls)
}

fn generated_hook(shell: &str, args: &[String], dir: &Path) -> PathBuf {
    let output = Command::new(devenv_bin())
        .args(["hook", shell, "--"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "devenv hook {shell} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = dir.join(format!("hook.{shell}"));
    fs::write(&path, output.stdout).unwrap();
    path
}

fn nul_separated_args(path: &Path) -> Vec<String> {
    let bytes = fs::read(path).unwrap();
    let mut args = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    assert_eq!(args.pop(), Some(&[][..]), "recording must end in NUL");
    args.into_iter()
        .map(|arg| String::from_utf8(arg.to_vec()).unwrap())
        .collect()
}

/// Shell name, the hook snippet that activates devenv, and a builder for the
/// shell's PATH-override line.
type ShellCase = (&'static str, String, fn(&Path) -> String);

/// bash, zsh, fish — sufficiently similar that one template covers all three.
fn shells() -> Vec<ShellCase> {
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

#[test]
fn forwarded_shell_args_preserve_boundaries_in_every_shell() {
    let project = fake_project();
    let marker = project.path().join("argument-was-evaluated");
    let args = vec![
        "--no-tui".to_string(),
        "--option".to_string(),
        "example.value:string".to_string(),
        format!(
            "spaces 'single' \"double\" $HOME ; $(touch {}) \\\nnewline",
            marker.display()
        ),
        String::new(),
        "echo".to_string(),
        "--literal-command-arg".to_string(),
    ];

    for shell in ["bash", "zsh", "fish", "nu"] {
        if !have(shell) {
            continue;
        }
        let hook_dir = tempfile::tempdir().unwrap();
        let hook = generated_hook(shell, &args, hook_dir.path());
        let (shim_dir, calls) = argv_recording_devenv_shim(project.path());
        let script = match shell {
            "fish" => format!(
                "set -e DEVENV_ROOT; set -e _DEVENV_HOOK_DIR\n\
                 source {hook:?}\n\
                 {path}\n\
                 cd {project:?}\n\
                 _devenv_hook\n",
                path = fish_path_override(shim_dir.path()),
                project = project.path(),
            ),
            "nu" => format!(
                "hide-env -i DEVENV_ROOT\nhide-env -i _DEVENV_HOOK_DIR\n\
                 source {hook:?}\n\
                 $env.PATH = ($env.PATH | prepend {shim:?})\n\
                 cd {project:?}\n\
                 _devenv_hook\n",
                shim = shim_dir.path(),
                project = project.path(),
            ),
            _ => format!(
                "unset DEVENV_ROOT _DEVENV_HOOK_DIR\n\
                 source {hook:?}\n\
                 {path}\n\
                 cd {project:?}\n\
                 _devenv_hook\n",
                path = posix_path_override(shim_dir.path()),
                project = project.path(),
            ),
        };

        let output = run(shell, &script);
        assert!(
            output.status.success(),
            "[{shell}] generated hook failed.\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        let mut expected = vec!["shell".to_string()];
        expected.extend(args.iter().cloned());
        assert_eq!(
            nul_separated_args(&calls),
            expected,
            "[{shell}] forwarded arguments changed"
        );
        assert!(
            !marker.exists(),
            "[{shell}] evaluated shell syntax from a forwarded argument"
        );
    }
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
fn hook_passes_invoking_shell_as_hint() {
    for (shell, src, path_override) in shells() {
        let project = fake_project();
        let shim_dir = tempfile::tempdir().unwrap();
        let calls = shim_dir.path().join("calls");
        let shim = shim_dir.path().join("devenv");
        fs::write(
            &shim,
            format!(
                r#"#!/bin/sh
case "$1" in
  hook-should-activate)
    printf '%s\n' {project:?}
    ;;
  shell)
    printf '%s\n' "${{_DEVENV_SHELL_HINT:-}}" > {calls:?}
    ;;
esac
"#,
                project = project.path(),
            ),
        )
        .unwrap();
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();

        let stale_shell = if shell == "fish" {
            "set -gx SHELL /bin/bash"
        } else {
            "export SHELL=/bin/bash"
        };
        let clear_markers = if shell == "fish" {
            "set -e DEVENV_ROOT; set -e _DEVENV_HOOK_DIR"
        } else {
            "unset DEVENV_ROOT _DEVENV_HOOK_DIR"
        };
        let script = format!(
            "{clear_markers}\n{stale_shell}\n{src}\ncd {project:?}\n{path_override}\n_devenv_hook\n",
            project = project.path(),
            path_override = path_override(shim_dir.path()),
        );
        let out = run(shell, &script);
        assert!(
            out.status.success(),
            "[{shell}] hook failed.\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        assert_eq!(
            fs::read_to_string(&calls).unwrap_or_default().trim(),
            shell,
            "[{shell}] hook did not pass the invoking shell as its hint"
        );
    }
}

#[test]
fn fish_activation_skips_if_already_active() {
    // `_devenv_hook_activate` is the final guard before spawning. It must
    // notice an existing environment and avoid stacking a redundant devenv
    // shell on top, regardless of how activation reached the helper.
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
  shell)
    printf 'shell %s\n' "$PWD" >> {calls:?}
    ;;
esac
"#,
        ),
    )
    .unwrap();
    fs::set_permissions(&shim_bin, fs::Permissions::from_mode(0o755)).unwrap();

    let bin = devenv_bin();
    let script = format!(
        "set -e DEVENV_ROOT; set -e _DEVENV_HOOK_DIR\n\
         {bin} hook fish | source\n\
         {po}\n\
         set -gx DEVENV_ROOT {root:?}\n\
         _devenv_hook_activate {root:?}\n\
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
        "fish spawned a redundant devenv shell even though DEVENV_ROOT was set.\n\
         Recorded:\n{recorded}",
    );
}

#[test]
fn fish_follow_cd_out_preserves_history_for_cd_dash() {
    // #2853: after the hook-spawned shell exits on cd-out, `_devenv_hook_activate`
    // follows the user to the target directory with `_devenv_builtin_cd_with_history`
    // (a `builtin cd`, not `cd`), to avoid re-triggering a user-overridden `cd`
    // (e.g. `zoxide init --cmd=cd`, which reported "infinite loop detected" on
    // this internal cd — see the fish hook's own comment above the call site).
    // Plain `builtin cd` bypasses fish's own directory-history bookkeeping too
    // though, since that lives in fish's bundled `cd` *function*, not a
    // PWD-change hook — so `cd -` right after silently skipped over the
    // project directory instead of returning to it.
    if !have("fish") {
        return;
    }
    let project_dir = fake_project();
    let other_dir = tempfile::tempdir().unwrap();

    let bin = devenv_bin();
    let script = format!(
        "{bin} hook fish | source\n\
         cd {project_dir:?}\n\
         _devenv_builtin_cd_with_history {other_dir:?}\n\
         cd -\n\
         echo AFTER_CD_DASH=$PWD\n",
        project_dir = project_dir.path(),
        other_dir = other_dir.path(),
    );
    let out = run("fish", &script);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&format!("AFTER_CD_DASH={}", project_dir.path().display())),
        "fish `cd -` did not return to the project directory that \
         `_devenv_builtin_cd_with_history` left.\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// A shimmed `devenv` whose `shell` records the call and reports that the user
/// cd'd out to `target`, i.e. the hook-spawned shell left the project.
fn cd_out_shim(project: &Path, target: &Path) -> (tempfile::TempDir, PathBuf) {
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
    printf '%s' {target:?} > {project:?}/.devenv/exit-dir
    ;;
esac
"#,
            calls = calls,
            project = project.display().to_string(),
            target = target.display().to_string(),
        ),
    )
    .unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    (dir, calls)
}

#[test]
fn activates_again_after_returning_to_the_same_project() {
    // Re-entering the project you were just followed out of must activate
    // again. `_DEVENV_HOOK_ACTIVATED` suppresses a re-spawn only for the case
    // where you `exit` the shell and stay put; once the hook has followed you
    // out to `exit-dir` that guard has to be cleared, or cd-ing back in is
    // silently ignored until you leave and return a second time.
    let parent = tempfile::tempdir().unwrap();
    let project = parent.path().join("project");

    for (shell, src) in posix_shells() {
        fs::create_dir_all(project.join(".devenv")).unwrap();
        let (_bin_dir, calls) = cd_out_shim(&project, parent.path());
        let script = format!(
            "unset DEVENV_ROOT _DEVENV_HOOK_DIR\n\
             {src}\n\
             {po}\n\
             cd {project:?}\n\
             _devenv_hook\n\
             cd {project:?}\n\
             _devenv_hook\n",
            po = posix_path_override(calls.parent().unwrap()),
        );
        let out = run(shell, &script);
        let recorded = fs::read_to_string(&calls).unwrap_or_default();
        assert_eq!(
            recorded.lines().count(),
            2,
            "[{shell}] cd-ing back into the project after being followed out did \
             not re-activate.\nRecorded:\n{recorded}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

#[test]
fn nu_activates_again_after_returning_to_the_same_project() {
    if !have("nu") {
        return;
    }
    let parent = tempfile::tempdir().unwrap();
    let project = parent.path().join("project");
    fs::create_dir_all(project.join(".devenv")).unwrap();
    let (_bin_dir, calls) = cd_out_shim(&project, parent.path());

    let hook_dir = tempfile::tempdir().unwrap();
    let hook_path = hook_dir.path().join("hook.nu");
    let hook_gen = Command::new(devenv_bin())
        .args(["hook", "nu"])
        .output()
        .unwrap();
    assert!(hook_gen.status.success(), "devenv hook nu failed");
    fs::write(&hook_path, &hook_gen.stdout).unwrap();

    let script = format!(
        "hide-env -i DEVENV_ROOT\nhide-env -i _DEVENV_HOOK_DIR\n\
         source {hook:?}\n\
         $env.PATH = ($env.PATH | prepend {bin_dir:?})\n\
         cd {project:?}\n_devenv_hook\n\
         cd {project:?}\n_devenv_hook\n",
        hook = hook_path,
        bin_dir = calls.parent().unwrap(),
    );
    let out = Command::new("nu").arg("-c").arg(&script).output().unwrap();
    let recorded = fs::read_to_string(&calls).unwrap_or_default();
    assert_eq!(
        recorded.lines().count(),
        2,
        "[nu] cd-ing back into the project after being followed out did not \
         re-activate.\nRecorded:\n{recorded}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr),
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
    // Not reaching the end of the script is necessary but not sufficient: a
    // bare `exit` from inside a hook throws `ShellError::Exit`, which only the
    // REPL top level handles. Under `nu -c` that unwinds the script and exits
    // 0, but a real interactive shell reports "Exit doesn't catch internally"
    // and stays alive (#3033). The parent hook is waiting on the process, so
    // require death by signal — which `exit` can never produce.
    assert_eq!(
        out.status.signal(),
        Some(15),
        "[nu] inner shell unwound the script instead of terminating the process \
         (status: {:?}).\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
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
