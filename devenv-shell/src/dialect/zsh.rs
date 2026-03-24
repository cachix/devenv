use super::{InteractiveArgs, RcfileContext, ShellDialect};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Zsh shell dialect implementation.
///
/// Architecture: We always launch bash first to source the devenv environment
/// (which produces bash syntax). The bash rcfile computes the env diff, saves
/// ZDOTDIR, sets ZDOTDIR to our init directory, then execs into zsh.
/// Our .zshrc restores the original ZDOTDIR and sources the user's .zshrc.
pub struct ZshDialect;

impl ShellDialect for ZshDialect {
    fn name(&self) -> &str {
        "zsh"
    }

    fn interactive_args(&self) -> InteractiveArgs {
        // We always launch bash first, then exec into zsh from the rcfile.
        super::BashDialect.interactive_args()
    }

    fn rcfile_content(&self, ctx: &RcfileContext) -> String {
        let target_shell = ctx.target_shell_path.unwrap_or("zsh");
        let zsh_dir = ctx.init_dir.join("zsh");
        let zsh_dir_str = zsh_dir.to_string_lossy();

        format!(
            r#"# Disable history during init so devenv internal commands don't pollute history.
set +o history

# Environment diff helpers (always defined for tracking)
{env_diff_helpers}

# Capture environment BEFORE sourcing devenv (for diff tracking)
_devenv_before_file=$(mktemp)
__devenv_capture_env > "$_devenv_before_file"

# Source the devenv environment
source "{env_script_path}"

# Compute and store the initial diff in _DEVENV_DIFF env var
__devenv_compute_diff "$_devenv_before_file"
rm -f "$_devenv_before_file"
unset _devenv_before_file

# Save PATH before zsh init potentially modifies it
export _DEVENV_PATH="$PATH"

# Save original ZDOTDIR so zsh init can restore it
if [ -n "$ZDOTDIR" ]; then
    export _DEVENV_REAL_ZDOTDIR="$ZDOTDIR"
fi

# Point ZDOTDIR to our init directory containing our .zshrc
export ZDOTDIR="{zsh_dir}"

# Re-enable history before exec
set -o history

# Exec into zsh (resolve via PATH if not absolute, since the devenv
# environment may have added it after this process started)
if [ ! -x "{target_shell}" ] && ! command -v "{target_shell}" >/dev/null 2>&1; then
    echo "devenv: error: shell '{target_shell}' not found" >&2
    echo "devenv: add zsh to your devenv.nix packages or set SHELL to an absolute path" >&2
    exit 1
fi
exec "{target_shell}" -i
echo "devenv: error: failed to exec into {target_shell}" >&2
exit 1
"#,
            env_diff_helpers = ctx.env_diff_helpers,
            env_script_path = ctx.env_script_path.to_string_lossy(),
            zsh_dir = zsh_dir_str,
            target_shell = target_shell,
        )
    }

    fn env_diff_helpers(&self) -> &str {
        // Reuse the same bash helpers, they are sourced in bash before exec to zsh
        super::BashDialect.env_diff_helpers()
    }

    fn reload_hook(&self, reload_file: &Path) -> String {
        // Zsh reload hook using precmd and zle widget
        format!(
            r#"
autoload -Uz add-zsh-hook

__devenv_reload_apply() {{
    # Source new environment if a reload is pending
    if [ -f "{reload_file}" ]; then
        # Shell out to bash to handle the env diff (bash syntax)
        local reload_output
        reload_output=$(bash -c '
            {bash_reload_script}
        ' 2>/dev/null)

        # Apply the environment changes
        if [ -n "$reload_output" ]; then
            eval "$reload_output"
        fi

        # Update saved PATH
        _DEVENV_PATH="$PATH"
    fi
}}

__devenv_restore_path() {{
    # Restore devenv PATH (in case direnv or other tools modified it)
    export PATH="$_DEVENV_PATH"
}}

add-zsh-hook precmd __devenv_restore_path

# Keybinding for manual reload
__devenv_reload_widget() {{
    __devenv_reload_apply
    zle reset-prompt
}}
zle -N __devenv_reload_widget
bindkey "${{DEVENV_RELOAD_KEYBIND:-\\e\\C-r}}" __devenv_reload_widget
"#,
            reload_file = reload_file.to_string_lossy(),
            bash_reload_script = super::bash_reload_subprocess_script(
                super::BashDialect.env_diff_helpers(),
                &reload_file.to_string_lossy(),
            ),
        )
    }

    fn user_rcfile(&self) -> Option<PathBuf> {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".zshrc"))
    }

    fn prompt_prefix(&self) -> &str {
        r#"PROMPT="(devenv) ${PROMPT}""#
    }

    fn format_task_exports(&self, exports: &BTreeMap<String, String>) -> String {
        // Zsh shares POSIX export syntax with bash
        let mut result = String::with_capacity(exports.len() * 50);
        for (key, value) in exports {
            result.push_str("export ");
            result.push_str(&shell_escape::escape(Cow::Borrowed(key)));
            result.push('=');
            result.push_str(&shell_escape::escape(Cow::Borrowed(value)));
            result.push('\n');
        }
        result
    }

    fn format_task_messages(&self, messages: &[String]) -> String {
        let mut result = String::with_capacity(messages.len() * 40);
        for msg in messages {
            result.push_str("printf '%s\\n' ");
            result.push_str(&shell_escape::escape(Cow::Borrowed(msg)));
            result.push('\n');
        }
        result
    }

    fn write_init_files(&self, ctx: &RcfileContext) -> std::io::Result<()> {
        let zsh_dir = ctx.init_dir.join("zsh");
        std::fs::create_dir_all(&zsh_dir)?;

        let reload_hook = ctx.reload_hook;
        let prompt_prefix = self.prompt_prefix();

        let zshrc_content = format!(
            r#"# devenv zsh init - restore ZDOTDIR and source user's .zshrc
if [ -n "$_DEVENV_REAL_ZDOTDIR" ]; then
    ZDOTDIR="$_DEVENV_REAL_ZDOTDIR"
    unset _DEVENV_REAL_ZDOTDIR
    [ -f "$ZDOTDIR/.zshrc" ] && source "$ZDOTDIR/.zshrc"
else
    unset ZDOTDIR
    [ -f "$HOME/.zshrc" ] && source "$HOME/.zshrc"
fi

# Restore devenv PATH after user's .zshrc may have modified it
export PATH="$_DEVENV_PATH"

# Set devenv prompt prefix
{prompt_prefix}

# Hot-reload hook
{reload_hook}
"#,
            prompt_prefix = prompt_prefix,
            reload_hook = reload_hook,
        );

        std::fs::write(zsh_dir.join(".zshrc"), zshrc_content)?;
        Ok(())
    }
}
