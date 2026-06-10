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
}
