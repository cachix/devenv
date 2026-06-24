//! Shell-hook regression tests across bash, zsh, fish, and nu. Each test
//! asserts one behavior; missing shells are skipped.
//!
//! - `outer_shell_survives_cd_out` — #2805
//! - `inner_shell_exits_on_cd_out` — hook-spawned shell must `exit` + write exit-dir
//! - `no_respawn_inside_devenv_shell` — follow-up to #2815
//! - `outer_hook_ignores_stale_exit_dir` — stale exit-dir without matching nonce is dropped
//! - `outer_hook_preserves_exit_dir_with_newline` — exit-dir paths may contain newlines

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

/// A `devenv` shim that activates `project_dir` and immediately returns from
/// `devenv shell` — so the outer hook reaches its exit-dir handling without
/// the inner shell having a chance to write a fresh exit-dir.
fn devenv_activate_shim(project_dir: &Path) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("devenv");
    fs::write(
        &bin,
        format!(
            "#!/usr/bin/env bash\ncase \"$1\" in\n  \
             hook-should-activate) printf '%s\\n' {project:?} ;;\n  \
             shell) exit 0 ;;\n  \
             *) exit 1 ;;\nesac\n",
            project = project_dir,
        ),
    )
    .unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    dir
}

/// A `devenv` shim that activates `project_dir`, then simulates the inner shell
/// having cd'd out to `$DEVENV_TEST_TARGET_DIR`.
fn devenv_cd_out_shim(project_dir: &Path) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("devenv");
    fs::write(
        &bin,
        format!(
            "#!/usr/bin/env bash\ncase \"$1\" in\n  \
             hook-should-activate) printf '%s\\n' {project:?} ;;\n  \
             shell) printf '%s\\n%s' \"$_DEVENV_EXIT_NONCE\" \"$DEVENV_TEST_TARGET_DIR\" \
                 > {project:?}/.devenv/exit-dir ;;\n  \
             *) exit 1 ;;\nesac\n",
            project = project_dir,
        ),
    )
    .unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    dir
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
    run_with_env(shell, script, &[])
}

fn run_with_env(shell: &str, script: &str, envs: &[(&str, &Path)]) -> std::process::Output {
    // Tests run under `devenv shell` so DEVENV_ROOT etc. are inherited;
    // strip them so each script controls its own activation state.
    let mut command = Command::new(shell);
    command
        .env_remove("DEVENV_ROOT")
        .env_remove("_DEVENV_HOOK_DIR")
        .env_remove("_DEVENV_HOOK_UNTRUSTED")
        .env_remove("_DEVENV_ACTIVATE_DIR")
        .env_remove("_DEVENV_EXIT_NONCE");
    for (key, value) in envs {
        command.env(*key, *value);
    }
    command.arg("-c").arg(script).output().unwrap()
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
    let nonce = "test-nonce-abc123";
    for (shell, src, _) in shells() {
        let tmp = fake_project();
        let script = format!(
            "export DEVENV_ROOT={root:?}\nexport _DEVENV_HOOK_DIR={root:?}\n\
             export _DEVENV_EXIT_NONCE={nonce}\n\
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
        assert_eq!(
            exit_dir,
            format!("{nonce}\n/"),
            "[{shell}] exit-dir should record nonce + cd target",
        );
    }
}

#[test]
fn inner_shell_skips_exit_dir_without_nonce() {
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
        assert!(
            !tmp.path().join(".devenv/exit-dir").exists(),
            "[{shell}] exit-dir written without a nonce (no parent hook to read it)",
        );
    }
}

#[test]
fn outer_hook_ignores_stale_exit_dir() {
    for (shell, src, path_override) in shells() {
        let tmp = fake_project();
        // A real directory the hook must refuse to follow into: it exists, so
        // the only thing preventing the cd is the nonce mismatch. This forces
        // the test through the nonce comparison instead of passing because the
        // recorded target happens to be missing.
        let stale_target = tempfile::tempdir().unwrap();
        let exit_dir = tmp.path().join(".devenv/exit-dir");
        // Well-formed exit-dir left by a prior session: a (now-stale) nonce
        // line followed by a path. The fresh activation's nonce won't match it.
        fs::write(
            &exit_dir,
            format!(
                "stale-nonce-from-prior-session\n{}",
                stale_target.path().display()
            ),
        )
        .unwrap();
        let shim_dir = devenv_activate_shim(tmp.path());
        let activate = if shell == "fish" {
            // Fish defers activation to `_devenv_hook_activate`, invoked from
            // the prompt event handler — call it directly in tests.
            format!("_devenv_hook_activate {:?}", tmp.path())
        } else {
            "_devenv_hook".to_string()
        };
        let script = format!(
            "cd {root:?}\n{src}\n{po}\n{activate}\npwd\n",
            root = tmp.path(),
            po = path_override(shim_dir.path()),
        );
        let out = run(shell, &script);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let final_pwd = stdout.lines().last().unwrap_or("");
        assert_eq!(
            final_pwd,
            tmp.path().to_string_lossy(),
            "[{shell}] stale exit-dir was honored even though nonce doesn't match.\n\
             stdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr),
        );
        assert!(
            !exit_dir.exists(),
            "[{shell}] stale exit-dir was not cleaned up after the outer hook ran",
        );
    }
}

#[test]
fn outer_hook_preserves_exit_dir_with_newline() {
    for (shell, src, path_override) in shells() {
        let tmp = fake_project();
        let target_parent = tempfile::tempdir().unwrap();
        let target_dir = target_parent.path().join("target-prefix\nsuffix");
        let truncated_dir = target_parent.path().join("target-prefix");
        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&truncated_dir).unwrap();

        let shim_dir = devenv_cd_out_shim(tmp.path());
        let activate = if shell == "fish" {
            // Fish defers activation to `_devenv_hook_activate`, invoked from
            // the prompt event handler — call it directly in tests.
            format!("_devenv_hook_activate {:?}", tmp.path())
        } else {
            "_devenv_hook".to_string()
        };
        let check = if shell == "fish" {
            r#"if test "$PWD" = "$DEVENV_TEST_TARGET_DIR"
    echo MATCH
else
    printf 'PWD=<%s>\nTARGET=<%s>\n' "$PWD" "$DEVENV_TEST_TARGET_DIR"
    exit 1
end"#
        } else {
            r#"if [[ "$PWD" == "$DEVENV_TEST_TARGET_DIR" ]]; then
    echo MATCH
else
    printf 'PWD=<%s>\nTARGET=<%s>\n' "$PWD" "$DEVENV_TEST_TARGET_DIR"
    exit 1
fi"#
        };
        let script = format!(
            "cd {root:?}\n{src}\n{po}\n{activate}\n{check}\n",
            root = tmp.path(),
            po = path_override(shim_dir.path()),
        );
        let out = run_with_env(
            shell,
            &script,
            &[("DEVENV_TEST_TARGET_DIR", target_dir.as_path())],
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("MATCH"),
            "[{shell}] exit-dir target containing a newline was not preserved.\n\
             stdout: {stdout}\nstderr: {}",
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

// Nu's env_change.PWD hook only fires in interactive sessions, so nu tests
// source the hook (which defines `_devenv_hook`) and call it directly.
// Different enough syntactically from the posix shells that folding it into
// the loop above forces more abstraction than it saves.

fn run_nu(setup: &str, body: &str) -> std::process::Output {
    run_nu_with_env(setup, body, &[])
}

fn run_nu_with_env(setup: &str, body: &str, envs: &[(&str, &Path)]) -> std::process::Output {
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
    let mut command = Command::new("nu");
    command
        .env_remove("DEVENV_ROOT")
        .env_remove("_DEVENV_HOOK_DIR")
        .env_remove("_DEVENV_HOOK_UNTRUSTED")
        .env_remove("_DEVENV_ACTIVATE_DIR")
        .env_remove("_DEVENV_EXIT_NONCE");
    for (key, value) in envs {
        command.env(*key, *value);
    }
    command.arg("-c").arg(&script).output().unwrap()
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
    let nonce = "test-nonce-abc123";
    let tmp = fake_project();
    let out = run_nu(
        &format!(
            r#"$env.DEVENV_ROOT = "{root}"; $env._DEVENV_HOOK_DIR = "{root}"; $env._DEVENV_EXIT_NONCE = "{nonce}""#,
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
    assert_eq!(
        exit_dir,
        format!("{nonce}\n/"),
        "[nu] exit-dir should record nonce + cd target",
    );
}

#[test]
fn nu_inner_shell_skips_exit_dir_without_nonce() {
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
    assert!(
        !tmp.path().join(".devenv/exit-dir").exists(),
        "[nu] exit-dir written without a nonce (no parent hook to read it)",
    );
}

#[test]
fn nu_outer_hook_ignores_stale_exit_dir() {
    if !have("nu") {
        return;
    }
    let tmp = fake_project();
    // Existing target: only the nonce mismatch may stop the cd (see the posix
    // twin `outer_hook_ignores_stale_exit_dir` for the rationale).
    let stale_target = tempfile::tempdir().unwrap();
    let exit_dir = tmp.path().join(".devenv/exit-dir");
    fs::write(
        &exit_dir,
        format!(
            "stale-nonce-from-prior-session\n{}",
            stale_target.path().display()
        ),
    )
    .unwrap();
    let shim_dir = devenv_activate_shim(tmp.path());
    let setup = format!(
        r#"$env.PATH = ($env.PATH | prepend "{shim}")"#,
        shim = shim_dir.path().display(),
    );
    let body = format!("cd {root:?}; _devenv_hook; pwd", root = tmp.path());
    let out = run_nu(&setup, &body);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let final_pwd = stdout.lines().last().unwrap_or("");
    assert_eq!(
        final_pwd,
        tmp.path().to_string_lossy(),
        "[nu] stale exit-dir was honored even though nonce doesn't match.\n\
         stdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        !exit_dir.exists(),
        "[nu] stale exit-dir was not cleaned up after the outer hook ran",
    );
}

#[test]
fn nu_outer_hook_preserves_exit_dir_with_newline() {
    if !have("nu") {
        return;
    }
    let tmp = fake_project();
    let target_parent = tempfile::tempdir().unwrap();
    let target_dir = target_parent.path().join("target-prefix\nsuffix");
    let truncated_dir = target_parent.path().join("target-prefix");
    fs::create_dir_all(&target_dir).unwrap();
    fs::create_dir_all(&truncated_dir).unwrap();

    let shim_dir = devenv_cd_out_shim(tmp.path());
    let setup = format!(
        r#"$env.PATH = ($env.PATH | prepend "{shim}")"#,
        shim = shim_dir.path().display(),
    );
    let body = format!(
        r#"cd {root:?}; _devenv_hook; if $env.PWD == $env.DEVENV_TEST_TARGET_DIR {{ print "MATCH" }} else {{ print $"PWD=($env.PWD) TARGET=($env.DEVENV_TEST_TARGET_DIR)"; exit 1 }}"#,
        root = tmp.path(),
    );
    let out = run_nu_with_env(
        &setup,
        &body,
        &[("DEVENV_TEST_TARGET_DIR", target_dir.as_path())],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("MATCH"),
        "[nu] exit-dir target containing a newline was not preserved.\n\
         stdout: {stdout}\nstderr: {}",
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
