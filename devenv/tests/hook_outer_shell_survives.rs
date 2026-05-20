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

type PathOverrideFn = fn(&Path) -> String;
type ShellSnippetFn = fn() -> &'static str;

/// Available shells with the syntax needed to (a) source the hook from the
/// real `devenv` binary and (b) override `PATH`.
fn shells() -> Vec<(&'static str, String, PathOverrideFn, ShellSnippetFn)> {
    let bash_src = format!(r#"eval "$({} hook bash)""#, devenv_bin());
    let fish_src = format!("{} hook fish | source", devenv_bin());
    [
        (
            "bash",
            bash_src,
            bash_path_override as PathOverrideFn,
            bash_block_rm as ShellSnippetFn,
        ),
        (
            "fish",
            fish_src,
            fish_path_override as PathOverrideFn,
            fish_block_rm as ShellSnippetFn,
        ),
    ]
    .into_iter()
    .filter(|(s, _, _, _)| have(s))
    .collect()
}

fn bash_path_override(dir: &Path) -> String {
    format!(r#"export PATH="{}:$PATH""#, dir.display())
}

fn fish_path_override(dir: &Path) -> String {
    format!("set -gx PATH {:?} $PATH", dir)
}

fn bash_block_rm() -> &'static str {
    "rm() { echo BLOCKED >&2; }"
}

fn fish_block_rm() -> &'static str {
    "function rm; echo BLOCKED >&2; end"
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
    Command::new(shell)
        .env_remove("DEVENV_ROOT")
        .env_remove("_DEVENV_HOOK_DIR")
        .env_remove("_DEVENV_HOOK_UNTRUSTED")
        .env_remove("_DEVENV_ACTIVATE_DIR")
        .arg("-c")
        .arg(script)
        .output()
        .unwrap()
}

#[test]
fn outer_shell_survives_cd_out() {
    for (shell, src, _, _) in shells() {
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
    for (shell, src, _, _) in shells() {
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
    for (shell, src, path_override, _) in shells() {
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

#[test]
fn stale_exit_dir_cleanup_bypasses_shell_rm_overrides() {
    for (shell, src, path_override, block_rm) in shells() {
        let tmp = fake_project();
        let exit_dir = tmp.path().join(".devenv/exit-dir");

        // Shim `devenv` on PATH so the hook activates the temp project and
        // exits the child shell immediately without writing a new exit-dir.
        let bin_dir = tempfile::tempdir().unwrap();
        let fake = bin_dir.path().join("devenv");
        fs::write(
            &fake,
            format!(
                "#!/usr/bin/env bash\ncase \"$1\" in\n  hook-should-activate) printf '%s\\n' {:?} ;;\n  shell) exit 0 ;;\n  *) exit 1 ;;\nesac\n",
                tmp.path(),
            ),
        )
        .unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();

        let activate = if shell == "fish" {
            format!("_devenv_hook_activate {:?}", tmp.path())
        } else {
            "_devenv_hook".to_string()
        };

        let script = format!(
            "cd {root:?}\nprintf '%s' / > {exit_dir:?}\n{src}\n{po}\n{block_rm}\n{activate}\npwd\n",
            root = tmp.path(),
            exit_dir = exit_dir,
            po = path_override(bin_dir.path()),
            block_rm = block_rm(),
            activate = activate,
        );
        let out = run(shell, &script);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout
                .lines()
                .any(|line| line == tmp.path().to_string_lossy()),
            "[{shell}] stale exit-dir changed directories even though rm was overridden.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr),
        );
        assert!(
            !exit_dir.exists(),
            "[{shell}] stale exit-dir was not removed when rm was overridden",
        );
    }
}
