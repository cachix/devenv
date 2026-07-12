//! Shell dialect abstraction for supporting different shell types (bash, zsh, fish, etc.).
//!
//! The [`ShellDialect`] trait encapsulates shell-specific behavior for interactive
//! shell sessions, including rcfile generation, environment diff tracking, reload
//! hooks, and launch arguments.

mod bash;
mod fish;
mod nushell;
mod zsh;

pub use bash::BashDialect;
pub use fish::FishDialect;
pub use nushell::NushellDialect;
pub use zsh::ZshDialect;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Shell-specific behavior for interactive sessions.
pub trait ShellDialect: Send + Sync {
    /// Shell name for display/logging (e.g., "bash", "zsh", "fish").
    fn name(&self) -> &str;

    /// Arguments to launch an interactive shell with a custom init script.
    /// Returns (prefix_args, suffix_args) that go around the rcfile path.
    /// e.g. bash: (["--noprofile", "--rcfile"], ["-i"])
    fn interactive_args(&self) -> InteractiveArgs;

    /// Generate the rcfile/init script content for an interactive shell.
    fn rcfile_content(&self, ctx: &RcfileContext) -> String;

    /// Generate environment diff helper functions (for hot-reload tracking).
    fn env_diff_helpers(&self) -> &str;

    /// Generate the hot-reload hook script (prompt hook).
    fn reload_hook(&self, reload_file: &Path) -> String;

    /// Path to the user's shell rc file (e.g., ~/.bashrc, ~/.zshrc).
    fn user_rcfile(&self) -> Option<PathBuf>;

    /// Generate a shell-specific PS1/prompt prefix for "(devenv)".
    fn prompt_prefix(&self) -> &str;

    /// Format task exports as shell export statements.
    ///
    /// Keys are already sorted (BTreeMap), giving deterministic output (important for direnv diffing).
    fn format_task_exports(&self, exports: &BTreeMap<String, String>) -> String;

    /// Format task messages as shell print statements.
    fn format_task_messages(&self, messages: &[String]) -> String;

    /// Write supplementary init files (e.g., zsh's ZDOTDIR .zshrc).
    /// Default implementation is a no-op (bash doesn't need extra files).
    fn write_init_files(&self, ctx: &RcfileContext) -> std::io::Result<()> {
        let _ = ctx;
        Ok(())
    }
}

/// Arguments for launching an interactive shell with a custom init script.
pub struct InteractiveArgs {
    /// Args before the rcfile path (e.g., `["--noprofile", "--rcfile"]` for bash).
    pub prefix: Vec<String>,
    /// Args after the rcfile path (e.g., `["-i"]` for bash).
    pub suffix: Vec<String>,
}

/// Look up a dialect by name, defaulting to bash if no match.
pub fn create_dialect(shell_name: &str) -> Box<dyn ShellDialect> {
    match shell_name {
        "zsh" => Box::new(ZshDialect),
        "fish" => Box::new(FishDialect),
        "nu" => Box::new(NushellDialect),
        "bash" => Box::new(BashDialect),
        other => {
            tracing::warn!(
                shell = other,
                "unrecognized shell dialect, falling back to bash"
            );
            Box::new(BashDialect)
        }
    }
}

/// Return `$XDG_CONFIG_HOME`, falling back to `$HOME/.config`.
pub(crate) fn xdg_config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

/// Generate the bash subprocess script used during hot-reload.
///
/// This script reverses the previous env diff, sources the new devenv
/// environment, computes a new diff, and outputs `export -p` for the
/// calling shell to parse.
///
/// The calling shell (zsh/fish) captures this script's stdout via command
/// substitution and then `eval`s it. Sourcing the devenv environment runs the
/// `shellHook` (i.e. `enterShell`), which prints to stdout. That output must
/// not leak into the captured `export -p` stream, otherwise the caller would
/// `eval` arbitrary `enterShell` output and hit shell parse errors. We redirect
/// the `source` output to the user's terminal so it is still displayed on
/// reload (matching bash's behavior), falling back to discarding it when no
/// controlling terminal is available.
pub(crate) fn bash_reload_subprocess_script(env_diff_helpers: &str, reload_file: &str) -> String {
    format!(
        r#"{env_diff_helpers}

# Reverse previous diff
__devenv_apply_reverse_diff

# Capture env before sourcing new devenv
_before=$(mktemp)
__devenv_capture_env > "$_before"

# Send enterShell output to the terminal instead of the captured stdout.
# Probe by actually opening /dev/tty: it can exist with writable permission
# bits yet fail to open (ENXIO) when there is no controlling terminal.
if {{ : >/dev/tty; }} 2>/dev/null; then
    _devenv_reload_out=/dev/tty
else
    _devenv_reload_out=/dev/null
fi

# Source new devenv environment
source "{reload_file}" >"$_devenv_reload_out" 2>"$_devenv_reload_out"
rm -f "{reload_file}"
unset _devenv_reload_out

# Compute new diff
__devenv_compute_diff "$_before"
rm -f "$_before"

# Output current environment for the calling shell to parse
export -p"#,
        env_diff_helpers = env_diff_helpers,
        reload_file = reload_file,
    )
}

/// bash/zsh function that `exit`s a hook-spawned shell when the user `cd`s
/// outside `$DEVENV_ROOT`. Shared verbatim by both dialects since the syntax
/// is plain POSIX, no zsh-specific features needed.
///
/// Resolves both `$PWD` and `$DEVENV_ROOT` through `cd -P` (builtin-only, no
/// `realpath` dependency) before comparing. `$PWD` preserves symlinks a user
/// navigated through (e.g. macOS's `/tmp` -> `/private/tmp`) while
/// `$DEVENV_ROOT` is canonicalized, so comparing the raw strings can
/// spuriously conclude the user left the project when they never did.
/// Falls back to the raw value if resolution fails (e.g. the directory was
/// removed out from under the shell).
pub(crate) fn exit_on_cd_out_snippet() -> &'static str {
    r#"__devenv_exit_on_cd_out() {
        local resolved_pwd resolved_root
        resolved_pwd=$(cd -P -- "$PWD" 2>/dev/null && pwd) || resolved_pwd="$PWD"
        resolved_root=$(cd -P -- "$DEVENV_ROOT" 2>/dev/null && pwd) || resolved_root="$DEVENV_ROOT"
        case "$resolved_pwd" in
            "$resolved_root"|"$resolved_root"/*) ;;
            *)
                printf '%s' "$PWD" > "$DEVENV_ROOT/.devenv/exit-dir"
                exit
                ;;
        esac
    }"#
}

/// Context passed to [`ShellDialect::rcfile_content`] for generating the init script.
pub struct RcfileContext<'a> {
    /// Path to the devenv environment script to source.
    pub env_script_path: &'a Path,
    /// Environment diff helper functions.
    pub env_diff_helpers: &'a str,
    /// Reload hook script (empty if no reload).
    pub reload_hook: &'a str,
    /// Path to the target shell binary (e.g., /usr/bin/zsh). None for bash (no exec needed).
    pub target_shell_path: Option<&'a str>,
    /// Directory for writing shell init files (e.g., .devenv/).
    pub init_dir: &'a Path,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    /// Regression test for https://github.com/cachix/devenv/issues/2919
    ///
    /// The non-bash reload path captures this subprocess's stdout and `eval`s it.
    /// Sourcing the devenv environment runs `enterShell` (via `eval "$shellHook"`),
    /// which prints to stdout. That output must not leak into the captured
    /// `export -p` stream, otherwise the caller `eval`s arbitrary `enterShell`
    /// output and hits a shell parse error (e.g. `(eval):6: parse error near '\n'`).
    #[test]
    fn reload_subprocess_does_not_leak_enter_shell_output() {
        // Simulate the activation script: `enterShell` prints to stdout and the
        // environment exports a variable that the reload must propagate.
        let reload_file =
            std::env::temp_dir().join(format!("devenv-reload-test-{}.sh", std::process::id()));
        std::fs::write(
            &reload_file,
            r#"echo "hello from devenv"
echo "GNU bash, version 5.3.9(1)-release (x86_64-pc-linux-gnu)"
echo ""
echo "License GPLv3+: GNU GPL version 3 or later <http://gnu.org/licenses/gpl.html>"
export DEVENV_RELOAD_TEST_VAR=reload_works
"#,
        )
        .expect("failed to write fake reload file");

        let script = bash_reload_subprocess_script(
            BashDialect.env_diff_helpers(),
            &reload_file.to_string_lossy(),
        );

        // Capture stdout exactly as the zsh/fish reload hook does (no tty here,
        // so `enterShell` output falls back to /dev/null).
        let output = Command::new("bash")
            .arg("-c")
            .arg(&script)
            .output()
            .expect("failed to run reload subprocess script");
        let stdout = String::from_utf8_lossy(&output.stdout);

        // The exported variable must reach the captured `export -p` output.
        assert!(
            stdout.contains("DEVENV_RELOAD_TEST_VAR"),
            "reload output should propagate exported variables, got:\n{stdout}"
        );

        // `enterShell` stdout must NOT leak into the captured output, otherwise
        // the caller's `eval` would choke on it.
        assert!(
            !stdout.contains("hello from devenv"),
            "enterShell output leaked into captured reload stdout:\n{stdout}"
        );
        assert!(
            !stdout.contains("License GPLv3+"),
            "enterShell output leaked into captured reload stdout:\n{stdout}"
        );

        // The captured output must be evaluable without a parse error, which is
        // the actual symptom reported in the issue.
        let eval = Command::new("bash")
            .arg("-c")
            .arg(stdout.as_ref())
            .output()
            .expect("failed to eval captured reload output");
        assert!(
            eval.status.success(),
            "evaluating captured reload output failed: {}",
            String::from_utf8_lossy(&eval.stderr)
        );

        let _ = std::fs::remove_file(&reload_file);
    }

    /// Regression tests for https://github.com/cachix/devenv/issues/2861
    ///
    /// A shell hook-spawned by `devenv shell` must `exit` when the user `cd`s
    /// outside `DEVENV_ROOT`, so the parent shell can follow. This must be
    /// handled directly in devenv's own generated init files (not just the
    /// user-loaded hook script), so it works regardless of whether the
    /// user's own rc file re-sources the hook for this non-login shell (a
    /// common fish idiom is to gate the whole rc behind `status is-login`,
    /// which a hook-spawned `fish -i` never satisfies).
    fn unique_tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("devenv-{name}-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn bash_rcfile_exits_on_cd_out_when_hook_spawned() {
        let tmp = unique_tmp_dir("bash-cdout");
        let root = tmp.join("project");
        std::fs::create_dir_all(root.join(".devenv")).unwrap();
        let empty_home = tmp.join("home");
        std::fs::create_dir_all(&empty_home).unwrap();
        // User prompt setup may replace PROMPT_COMMAND entirely. The devenv
        // cd-out handler must be installed after this bashrc is sourced.
        std::fs::write(
            empty_home.join(".bashrc"),
            "PROMPT_COMMAND='echo USER_PROMPT_COMMAND'\n",
        )
        .unwrap();

        let env_script = tmp.join("env.sh");
        std::fs::write(&env_script, format!("export DEVENV_ROOT={root:?}\n")).unwrap();

        let ctx = RcfileContext {
            env_script_path: &env_script,
            env_diff_helpers: BashDialect.env_diff_helpers(),
            reload_hook: "",
            target_shell_path: None,
            init_dir: &tmp,
        };
        let rcfile_path = tmp.join("rcfile.sh");
        std::fs::write(&rcfile_path, BashDialect.rcfile_content(&ctx)).unwrap();

        // PROMPT_COMMAND only runs before an interactive prompt, so evaluate
        // it explicitly under `bash -c`. This also verifies that sourcing the
        // user's bashrc above did not overwrite the devenv handler, and that
        // installing the handler did not discard the user's prompt command.
        let script = format!(
            "source {rcfile_path:?}\ncd \"$DEVENV_ROOT\"\neval \"$PROMPT_COMMAND\"\ncd /\neval \"$PROMPT_COMMAND\"\necho SHOULD_NOT_REACH\n"
        );
        let output = Command::new("bash")
            .env("HOME", &empty_home)
            .env("_DEVENV_HOOK_DIR", &root)
            .arg("-c")
            .arg(&script)
            .output()
            .expect("failed to run bash rcfile");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("USER_PROMPT_COMMAND"),
            "[bash] devenv discarded the user's PROMPT_COMMAND.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            !stdout.contains("SHOULD_NOT_REACH"),
            "[bash] hook-spawned rcfile did not exit on cd-out.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        let exit_dir = std::fs::read_to_string(root.join(".devenv/exit-dir")).unwrap();
        assert_eq!(exit_dir, "/", "exit-dir should record cd target");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn zsh_rcfile_exits_on_cd_out_when_hook_spawned() {
        if Command::new("zsh").arg("--version").output().is_err() {
            return;
        }
        let tmp = unique_tmp_dir("zsh-cdout");
        let root = tmp.join("project");
        std::fs::create_dir_all(root.join(".devenv")).unwrap();
        let init_dir = tmp.join("init");
        std::fs::create_dir_all(&init_dir).unwrap();
        let empty_home = tmp.join("home");
        std::fs::create_dir_all(&empty_home).unwrap();
        // User prompt setup may replace the precmd hook array entirely. The
        // devenv cd-out handler must be installed after this zshrc is sourced
        // without discarding the user's hook.
        std::fs::write(
            empty_home.join(".zshrc"),
            "user_precmd() { echo USER_PRECMD; }\nprecmd_functions=(user_precmd)\n",
        )
        .unwrap();

        let ctx = RcfileContext {
            env_script_path: Path::new("/dev/null"),
            env_diff_helpers: "",
            reload_hook: "",
            target_shell_path: None,
            init_dir: &init_dir,
        };
        ZshDialect.write_init_files(&ctx).unwrap();
        let zsh_dir = init_dir.join("zsh");

        // `precmd` only runs before an interactive prompt, so invoke its
        // registered functions explicitly under `zsh -c`. This also verifies
        // that sourcing the user's zshrc above did not discard the handler,
        // and that installing the handler preserved the user's hook.
        let script = "cd \"$DEVENV_ROOT\"\nfor hook in \"${precmd_functions[@]}\"; do \"$hook\"; done\ncd /\nfor hook in \"${precmd_functions[@]}\"; do \"$hook\"; done\necho SHOULD_NOT_REACH\n";
        let output = Command::new("zsh")
            .env("HOME", &empty_home)
            .env("ZDOTDIR", &zsh_dir)
            .env("DEVENV_ROOT", &root)
            .env("_DEVENV_HOOK_DIR", &root)
            .env("_DEVENV_PATH", std::env::var("PATH").unwrap_or_default())
            .arg("-i")
            .arg("-c")
            .arg(script)
            .output()
            .expect("failed to run zsh rcfile");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("USER_PRECMD"),
            "[zsh] devenv discarded the user's precmd hook.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            !stdout.contains("SHOULD_NOT_REACH"),
            "[zsh] hook-spawned rcfile did not exit on cd-out.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        let exit_dir = std::fs::read_to_string(root.join(".devenv/exit-dir")).unwrap();
        assert_eq!(exit_dir, "/", "exit-dir should record cd target");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fish_rcfile_exits_on_cd_out_when_hook_spawned() {
        if Command::new("fish").arg("--version").output().is_err() {
            return;
        }
        let tmp = unique_tmp_dir("fish-cdout");
        let root = tmp.join("project");
        std::fs::create_dir_all(root.join(".devenv")).unwrap();
        let init_dir = tmp.join("init");
        std::fs::create_dir_all(&init_dir).unwrap();

        let ctx = RcfileContext {
            env_script_path: Path::new("/dev/null"),
            env_diff_helpers: "",
            reload_hook: "",
            target_shell_path: None,
            init_dir: &init_dir,
        };
        FishDialect.write_init_files(&ctx).unwrap();
        let devenv_fish = init_dir.join("devenv.fish");

        let script = format!("source {devenv_fish:?}\ncd /\necho SHOULD_NOT_REACH\n");
        let output = Command::new("fish")
            .env("DEVENV_ROOT", &root)
            .env("_DEVENV_HOOK_DIR", &root)
            .env("_DEVENV_PATH", std::env::var("PATH").unwrap_or_default())
            .arg("-c")
            .arg(&script)
            .output()
            .expect("failed to run fish rcfile");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            !stdout.contains("SHOULD_NOT_REACH"),
            "[fish] hook-spawned rcfile did not exit on cd-out.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        let exit_dir = std::fs::read_to_string(root.join(".devenv/exit-dir")).unwrap();
        assert_eq!(exit_dir, "/", "exit-dir should record cd target");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn nu_rcfile_exits_on_cd_out_when_hook_spawned() {
        if Command::new("nu").arg("--version").output().is_err() {
            return;
        }
        let tmp = unique_tmp_dir("nu-cdout");
        let root = tmp.join("project");
        std::fs::create_dir_all(root.join(".devenv")).unwrap();
        let init_dir = tmp.join("init");
        std::fs::create_dir_all(&init_dir).unwrap();

        let ctx = RcfileContext {
            env_script_path: Path::new("/dev/null"),
            env_diff_helpers: "",
            reload_hook: "",
            target_shell_path: None,
            init_dir: &init_dir,
        };
        NushellDialect.write_init_files(&ctx).unwrap();
        let config_nu = init_dir.join("nu").join("config.nu");

        // Simulate a user config.nu replacing the PWD hook list with its own
        // callback. Do this in the generated file so the test does not depend
        // on process-global HOME/XDG_CONFIG_HOME state while files are
        // generated in parallel.
        let config = std::fs::read_to_string(&config_nu).unwrap();
        let user_config_marker =
            "# Source user's config.nu for their customizations (aliases, keybindings, etc.)\n";
        let marker_end = config.find(user_config_marker).unwrap() + user_config_marker.len();
        let source_line_end = marker_end + config[marker_end..].find('\n').unwrap() + 1;
        let mut config_with_user_hooks = String::with_capacity(config.len());
        config_with_user_hooks.push_str(&config[..marker_end]);
        config_with_user_hooks
            .push_str("$env.config.hooks.env_change.PWD = [{|| print USER_PWD_HOOK }]\n");
        config_with_user_hooks.push_str(&config[source_line_end..]);
        std::fs::write(&config_nu, config_with_user_hooks).unwrap();

        // PWD hooks only fire in truly interactive sessions, so invoke the
        // registered callback explicitly under `nu -c`. This verifies that
        // sourcing the user's config above did not discard the handler, and
        // that installing the handler preserved the user's callback.
        let script = format!(
            "source {config_nu:?}\ncd $env.DEVENV_ROOT\nfor hook in $env.config.hooks.env_change.PWD {{ do $hook }}\ncd /\nfor hook in $env.config.hooks.env_change.PWD {{ do $hook }}\nprint SHOULD_NOT_REACH\n"
        );
        let output = Command::new("nu")
            .env("DEVENV_ROOT", &root)
            .env("_DEVENV_HOOK_DIR", &root)
            .env("_DEVENV_PATH", std::env::var("PATH").unwrap_or_default())
            .arg("-c")
            .arg(&script)
            .output()
            .expect("failed to run nu rcfile");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("USER_PWD_HOOK"),
            "[nu] devenv discarded the user's PWD hook.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            !stdout.contains("SHOULD_NOT_REACH"),
            "[nu] hook-spawned rcfile did not exit on cd-out.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        let exit_dir = std::fs::read_to_string(root.join(".devenv/exit-dir")).unwrap();
        assert_eq!(exit_dir, "/", "exit-dir should record cd target");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // `_DEVENV_PREV_PWD` regression tests: a freshly-activated shell's `cd -`
    // must return to wherever the user was right before activation. Bash/zsh
    // discard an inherited OLDPWD unconditionally (verified directly against
    // a plain, non-devenv shell); fish's `cd -` uses `$dirprev`, a shell
    // variable, not OLDPWD, so it never crosses a process boundary either
    // way. Each dialect re-derives it from `_DEVENV_PREV_PWD` differently.

    #[test]
    fn bash_rcfile_seeds_oldpwd_from_prev_pwd() {
        let tmp = unique_tmp_dir("bash-prevpwd");
        let prev = tmp.join("prev");
        std::fs::create_dir_all(&prev).unwrap();
        let empty_home = tmp.join("home");
        std::fs::create_dir_all(&empty_home).unwrap();
        let env_script = tmp.join("env.sh");
        std::fs::write(&env_script, "").unwrap();

        let ctx = RcfileContext {
            env_script_path: &env_script,
            env_diff_helpers: BashDialect.env_diff_helpers(),
            reload_hook: "",
            target_shell_path: None,
            init_dir: &tmp,
        };
        let rcfile_path = tmp.join("rcfile.sh");
        std::fs::write(&rcfile_path, BashDialect.rcfile_content(&ctx)).unwrap();

        let script = format!("source {rcfile_path:?}\necho OLDPWD=$OLDPWD\n");
        let output = Command::new("bash")
            .env("HOME", &empty_home)
            .env("_DEVENV_PREV_PWD", &prev)
            .current_dir(&tmp)
            .arg("-c")
            .arg(&script)
            .output()
            .expect("failed to run bash rcfile");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(&format!("OLDPWD={}", prev.display())),
            "[bash] rcfile did not seed OLDPWD from _DEVENV_PREV_PWD.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn zsh_rcfile_seeds_oldpwd_from_prev_pwd() {
        if Command::new("zsh").arg("--version").output().is_err() {
            return;
        }
        let tmp = unique_tmp_dir("zsh-prevpwd");
        let prev = tmp.join("prev");
        std::fs::create_dir_all(&prev).unwrap();
        let init_dir = tmp.join("init");
        std::fs::create_dir_all(&init_dir).unwrap();
        let empty_home = tmp.join("home");
        std::fs::create_dir_all(&empty_home).unwrap();

        let ctx = RcfileContext {
            env_script_path: Path::new("/dev/null"),
            env_diff_helpers: "",
            reload_hook: "",
            target_shell_path: None,
            init_dir: &init_dir,
        };
        ZshDialect.write_init_files(&ctx).unwrap();
        let zsh_dir = init_dir.join("zsh");

        let output = Command::new("zsh")
            .env("HOME", &empty_home)
            .env("ZDOTDIR", &zsh_dir)
            .env("_DEVENV_PREV_PWD", &prev)
            .env("_DEVENV_PATH", std::env::var("PATH").unwrap_or_default())
            .current_dir(&tmp)
            .arg("-i")
            .arg("-c")
            .arg("echo OLDPWD=$OLDPWD\n")
            .output()
            .expect("failed to run zsh rcfile");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(&format!("OLDPWD={}", prev.display())),
            "[zsh] rcfile did not seed OLDPWD from _DEVENV_PREV_PWD.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fish_rcfile_seeds_dirprev_from_prev_pwd() {
        if Command::new("fish").arg("--version").output().is_err() {
            return;
        }
        let tmp = unique_tmp_dir("fish-prevpwd");
        let prev = tmp.join("prev");
        std::fs::create_dir_all(&prev).unwrap();
        let init_dir = tmp.join("init");
        std::fs::create_dir_all(&init_dir).unwrap();

        let ctx = RcfileContext {
            env_script_path: Path::new("/dev/null"),
            env_diff_helpers: "",
            reload_hook: "",
            target_shell_path: None,
            init_dir: &init_dir,
        };
        FishDialect.write_init_files(&ctx).unwrap();
        let devenv_fish = init_dir.join("devenv.fish");

        let script = format!("source {devenv_fish:?}\ncd -\npwd\n");
        let output = Command::new("fish")
            .env("_DEVENV_PREV_PWD", &prev)
            .env("_DEVENV_PATH", std::env::var("PATH").unwrap_or_default())
            .current_dir(&tmp)
            .arg("-c")
            .arg(&script)
            .output()
            .expect("failed to run fish rcfile");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.trim().ends_with(&prev.display().to_string()),
            "[fish] rcfile did not seed dirprev from _DEVENV_PREV_PWD; `cd -` did not return there.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn nu_rcfile_seeds_oldpwd_from_prev_pwd() {
        // Can't use real nu here: its interactive mode unconditionally
        // refuses non-tty stdin (pipe or file alike), so it can't run
        // headless in a test. Can't use a `/bin/sh` fake either: POSIX
        // shells discard their own inherited OLDPWD before running anything
        // (the exact behavior this fix works around), so a shell script
        // would "fail" for the wrong reason. Perl does neither.
        if Command::new("perl").arg("--version").output().is_err() {
            return;
        }
        let tmp = unique_tmp_dir("nu-prevpwd");
        let prev = tmp.join("prev");
        std::fs::create_dir_all(&prev).unwrap();
        let fake_nu = tmp.join("fake-nu");
        std::fs::write(
            &fake_nu,
            "#!/usr/bin/perl\nprint \"OLDPWD=$ENV{OLDPWD}\\n\";\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_nu, std::fs::Permissions::from_mode(0o755)).unwrap();

        let env_script = tmp.join("env.sh");
        std::fs::write(&env_script, "").unwrap();
        let init_dir = tmp.join("init");
        std::fs::create_dir_all(&init_dir).unwrap();

        let ctx = RcfileContext {
            env_script_path: &env_script,
            env_diff_helpers: NushellDialect.env_diff_helpers(),
            reload_hook: "",
            target_shell_path: Some(fake_nu.to_str().unwrap()),
            init_dir: &init_dir,
        };
        let rcfile_path = tmp.join("rcfile.sh");
        std::fs::write(&rcfile_path, NushellDialect.rcfile_content(&ctx)).unwrap();

        let script = format!("source {rcfile_path:?}\n");
        let output = Command::new("bash")
            .env("_DEVENV_PREV_PWD", &prev)
            .current_dir(&tmp)
            .arg("-c")
            .arg(&script)
            .output()
            .expect("failed to run nu rcfile");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(&format!("OLDPWD={}", prev.display())),
            "[nu] rcfile did not re-export OLDPWD from _DEVENV_PREV_PWD before exec.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A project directory reachable via a symlinked path (what `$PWD`
    /// preserves after a real `cd`) and its canonicalized form (what
    /// devenv's Rust side sets `DEVENV_ROOT` to). Creates the symlink itself
    /// rather than relying on the host temp dir already being one (e.g.
    /// macOS's /tmp -> /private/tmp), so the test is deterministic on any
    /// platform instead of silently skipping on ones where it isn't.
    fn symlinked_project_dirs(name: &str) -> (PathBuf, PathBuf) {
        let tmp = unique_tmp_dir(name);
        let real = tmp.join("real");
        std::fs::create_dir_all(real.join(".devenv")).unwrap();
        let symlinked = tmp.join("project");
        std::os::unix::fs::symlink(&real, &symlinked).unwrap();
        let canonical = std::fs::canonicalize(&symlinked).unwrap();
        (symlinked, canonical)
    }

    #[test]
    fn bash_rcfile_survives_cd_within_symlinked_project_root() {
        // macOS's /tmp is a symlink to /private/tmp (and similar
        // elsewhere). DEVENV_ROOT is canonicalized by devenv's Rust side,
        // while $PWD preserves whatever symlinked path the user actually
        // `cd`'d through — comparing the two as raw strings can spuriously
        // conclude the user left the project when they're still in it.
        let (symlinked_root, canonical_root) = symlinked_project_dirs("bash-symlink-cdout");
        let tmp = symlinked_root.parent().unwrap().to_path_buf();
        let empty_home = tmp.join("home");
        std::fs::create_dir_all(&empty_home).unwrap();

        let env_script = tmp.join("env.sh");
        std::fs::write(
            &env_script,
            format!("export DEVENV_ROOT={canonical_root:?}\n"),
        )
        .unwrap();

        let ctx = RcfileContext {
            env_script_path: &env_script,
            env_diff_helpers: BashDialect.env_diff_helpers(),
            reload_hook: "",
            target_shell_path: None,
            init_dir: &tmp,
        };
        let rcfile_path = tmp.join("rcfile.sh");
        std::fs::write(&rcfile_path, BashDialect.rcfile_content(&ctx)).unwrap();

        let script = format!(
            "source {rcfile_path:?}\ncd {symlinked_root:?}\n__devenv_exit_on_cd_out\necho SURVIVED\n"
        );
        let output = Command::new("bash")
            .env("HOME", &empty_home)
            .env("_DEVENV_HOOK_DIR", &canonical_root)
            .arg("-c")
            .arg(&script)
            .output()
            .expect("failed to run bash rcfile");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("SURVIVED"),
            "[bash] symlinked project root wrongly treated as cd-out.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            !canonical_root.join(".devenv/exit-dir").exists(),
            "[bash] exit-dir should not have been written"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn zsh_rcfile_survives_cd_within_symlinked_project_root() {
        if Command::new("zsh").arg("--version").output().is_err() {
            return;
        }
        let (symlinked_root, canonical_root) = symlinked_project_dirs("zsh-symlink-cdout");
        let tmp = symlinked_root.parent().unwrap().to_path_buf();
        let init_dir = tmp.join("init");
        std::fs::create_dir_all(&init_dir).unwrap();
        let empty_home = tmp.join("home");
        std::fs::create_dir_all(&empty_home).unwrap();

        let ctx = RcfileContext {
            env_script_path: Path::new("/dev/null"),
            env_diff_helpers: "",
            reload_hook: "",
            target_shell_path: None,
            init_dir: &init_dir,
        };
        ZshDialect.write_init_files(&ctx).unwrap();
        let zsh_dir = init_dir.join("zsh");

        let script = format!("cd {symlinked_root:?}\n__devenv_exit_on_cd_out\necho SURVIVED\n");
        let output = Command::new("zsh")
            .env("HOME", &empty_home)
            .env("ZDOTDIR", &zsh_dir)
            .env("DEVENV_ROOT", &canonical_root)
            .env("_DEVENV_HOOK_DIR", &canonical_root)
            .env("_DEVENV_PATH", std::env::var("PATH").unwrap_or_default())
            .arg("-i")
            .arg("-c")
            .arg(script)
            .output()
            .expect("failed to run zsh rcfile");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("SURVIVED"),
            "[zsh] symlinked project root wrongly treated as cd-out.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            !canonical_root.join(".devenv/exit-dir").exists(),
            "[zsh] exit-dir should not have been written"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fish_rcfile_survives_cd_within_symlinked_project_root() {
        if Command::new("fish").arg("--version").output().is_err() {
            return;
        }
        let (symlinked_root, canonical_root) = symlinked_project_dirs("fish-symlink-cdout");
        let tmp = symlinked_root.parent().unwrap().to_path_buf();
        let init_dir = tmp.join("init");
        std::fs::create_dir_all(&init_dir).unwrap();

        let ctx = RcfileContext {
            env_script_path: Path::new("/dev/null"),
            env_diff_helpers: "",
            reload_hook: "",
            target_shell_path: None,
            init_dir: &init_dir,
        };
        FishDialect.write_init_files(&ctx).unwrap();
        let devenv_fish = init_dir.join("devenv.fish");

        let script = format!("source {devenv_fish:?}\ncd {symlinked_root:?}\necho SURVIVED\n");
        let output = Command::new("fish")
            .env("DEVENV_ROOT", &canonical_root)
            .env("_DEVENV_HOOK_DIR", &canonical_root)
            .env("_DEVENV_PATH", std::env::var("PATH").unwrap_or_default())
            .arg("-c")
            .arg(&script)
            .output()
            .expect("failed to run fish rcfile");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("SURVIVED"),
            "[fish] symlinked project root wrongly treated as cd-out.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            !canonical_root.join(".devenv/exit-dir").exists(),
            "[fish] exit-dir should not have been written"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn nu_rcfile_survives_cd_within_symlinked_project_root() {
        if Command::new("nu").arg("--version").output().is_err() {
            return;
        }
        let (symlinked_root, canonical_root) = symlinked_project_dirs("nu-symlink-cdout");
        let tmp = symlinked_root.parent().unwrap().to_path_buf();
        let init_dir = tmp.join("init");
        std::fs::create_dir_all(&init_dir).unwrap();

        let ctx = RcfileContext {
            env_script_path: Path::new("/dev/null"),
            env_diff_helpers: "",
            reload_hook: "",
            target_shell_path: None,
            init_dir: &init_dir,
        };
        NushellDialect.write_init_files(&ctx).unwrap();
        let config_nu = init_dir.join("nu").join("config.nu");

        let script = format!(
            "source {config_nu:?}\ncd {symlinked_root:?}\n_devenv_shell_exit_on_cd_out\nprint SURVIVED\n"
        );
        let output = Command::new("nu")
            .env("DEVENV_ROOT", &canonical_root)
            .env("_DEVENV_HOOK_DIR", &canonical_root)
            .env("_DEVENV_PATH", std::env::var("PATH").unwrap_or_default())
            .arg("-c")
            .arg(&script)
            .output()
            .expect("failed to run nu rcfile");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("SURVIVED"),
            "[nu] symlinked project root wrongly treated as cd-out.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            !canonical_root.join(".devenv/exit-dir").exists(),
            "[nu] exit-dir should not have been written"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
