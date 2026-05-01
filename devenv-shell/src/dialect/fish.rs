use super::{InteractiveArgs, RcfileContext, ShellDialect};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Fish shell dialect implementation.
///
/// Architecture: We always launch bash first to source the devenv environment
/// (which produces bash syntax). The bash rcfile computes the env diff, then
/// execs into fish, sourcing our configuration, which sets up devenv integration.
pub struct FishDialect;

impl ShellDialect for FishDialect {
    fn name(&self) -> &str {
        "fish"
    }

    fn interactive_args(&self) -> InteractiveArgs {
        // We always launch bash first, then exec into fish from the rcfile.
        super::BashDialect.interactive_args()
    }

    fn rcfile_content(&self, ctx: &RcfileContext) -> String {
        let target_shell = ctx.target_shell_path.unwrap_or("fish");

        format!(
            r#"# Disable history during init so devenv internal commands do not pollute history.
set +o history

# Environment diff helpers (always defined for tracking)
{env_diff_helpers}

# Capture environment BEFORE sourcing devenv (for diff tracking)
_devenv_before_file=$(mktemp)
__devenv_capture_env > "$_devenv_before_file"

# Source the devenv environment
source "{env_script_path}"

# Restore SHELL to the target shell (Nix env sets it to /nix/store/.../bash).
# Resolve to absolute path via PATH in case the devenv env provides the shell.
export SHELL="$(command -v "{target_shell}")"

# Compute and store the initial diff in _DEVENV_DIFF env var
__devenv_compute_diff "$_devenv_before_file"
rm -f "$_devenv_before_file"
unset _devenv_before_file

# Save PATH before fish init potentially modifies it
export _DEVENV_PATH="$PATH"

# Exec into fish (resolve via PATH if not absolute, since the devenv
# environment may have added it after this process started)
if [ ! -x "{target_shell}" ] && ! command -v "{target_shell}" >/dev/null 2>&1; then
    echo "devenv: error: shell '{target_shell}' not found" >&2
    echo "devenv: add fish to your devenv.nix packages or set SHELL to an absolute path" >&2
    exit 1
fi
exec "{target_shell}" -i -C "source {init_dir}/devenv.fish"
echo "devenv: error: failed to exec into {target_shell}" >&2
exit 1
"#,
            env_diff_helpers = ctx.env_diff_helpers,
            env_script_path = ctx.env_script_path.to_string_lossy(),
            init_dir = ctx.init_dir.to_string_lossy(),
            target_shell = target_shell,
        )
    }

    fn env_diff_helpers(&self) -> &str {
        // Reuse the same bash helpers, they are sourced in bash before exec to fish
        super::BashDialect.env_diff_helpers()
    }

    fn reload_hook(&self, reload_file: &Path) -> String {
        // Fish reload hook using key binding and pre-prompt path restore.
        //
        // For reload, we shell out to bash to handle the env diff
        // (which uses bash syntax), then parse the `export -p` output
        // to apply changes via `set -gx` in fish.
        format!(
            r#"
function __devenv_reload_apply
    # Source new environment if a reload is pending
    if test -f "{reload_file}"
        # Shell out to bash to handle the env diff (bash syntax).
        # The bash subprocess inherits our current environment, reverses
        # the previous diff, sources the new devenv env, computes a new
        # diff, and outputs the resulting environment via export -p.
        set -l reload_output (bash -c '
            {bash_reload_script}
        ' 2>/dev/null)

        # Parse bash export -p output (declare -x VAR="value") and apply
        # each variable to the fish environment via set -gx.
        for line in $reload_output
            # Match lines of the form: declare -x VAR="value"
            set -l parts (string match -r '^declare -x ([^=]+)="(.*)"$' -- $line)
            if test (count $parts) -gt 0
                set -l var $parts[2]
                set -l val $parts[3]
                # Skip fish read-only electric variables that bash exports.
                # PWD and SHLVL are managed by fish; assigning to them errors.
                if test "$var" = PWD -o "$var" = SHLVL
                    continue
                end
                # PATH, MANPATH, CDPATH are list variables in fish;
                # split colon-separated values into proper lists.
                if test "$var" = PATH -o "$var" = MANPATH -o "$var" = CDPATH
                    set -gx $var (string split ":" -- $val)
                else
                    set -gx $var $val
                end
            else
                # Variable exported without a value: declare -x VAR
                set -l noval (string match -r '^declare -x ([^=]+)$' -- $line)
                if test (count $noval) -gt 0
                    set -gx $noval[2] ""
                end
            end
        end

        # Update saved PATH
        set -gx _DEVENV_PATH (string join ":" -- $PATH)
    end
end

function __devenv_restore_path
    # Restore devenv PATH (in case direnv or other tools modified it).
    # _DEVENV_PATH is a colon-separated string; split into a list for fish.
    set -gx PATH (string split ":" -- $_DEVENV_PATH)
end

# Keybinding for manual reload (Ctrl+Alt+R by default)
function __devenv_reload_keybind_handler
    __devenv_reload_apply
    commandline -f repaint
end
bind \e\cr __devenv_reload_keybind_handler
"#,
            reload_file = reload_file.to_string_lossy(),
            bash_reload_script = super::bash_reload_subprocess_script(
                super::BashDialect.env_diff_helpers(),
                &reload_file.to_string_lossy(),
            ),
        )
    }

    fn user_rcfile(&self) -> Option<PathBuf> {
        super::xdg_config_home().map(|p| p.join("fish").join("config.fish"))
    }

    fn prompt_prefix(&self) -> &str {
        // Fish uses functions for prompts. We prepend (devenv) by wrapping
        // fish_prompt in write_init_files, so we return an empty string here.
        ""
    }

    fn format_task_exports(&self, exports: &BTreeMap<String, String>) -> String {
        let mut result = String::with_capacity(exports.len() * 50);
        for (key, value) in exports {
            // Fish: set -gx KEY VALUE (no = sign, value is separate argument)
            result.push_str("set -gx ");
            result.push_str(&shell_escape::escape(Cow::Borrowed(key)));
            result.push(' ');
            // Fish double-quoting: escape backslashes and double quotes.
            // Fish single-quoted strings cannot contain single quotes at all,
            // so we must use double-quoted strings.
            result.push('"');
            result.push_str(&value.replace('\\', "\\\\").replace('"', "\\\""));
            result.push('"');
            result.push('\n');
        }
        result
    }

    fn format_task_messages(&self, messages: &[String]) -> String {
        let mut result = String::with_capacity(messages.len() * 40);
        for msg in messages {
            // Fish double-quoting: escape backslashes and double quotes.
            result.push_str("echo \"");
            result.push_str(&msg.replace('\\', "\\\\").replace('"', "\\\""));
            result.push_str("\"\n");
        }
        result
    }

    fn write_init_files(&self, ctx: &RcfileContext) -> std::io::Result<()> {
        let reload_hook = ctx.reload_hook;

        // When reload is enabled, call both reload-apply and path-restore
        // before each prompt. This matches bash's PROMPT_COMMAND and zsh's
        // precmd behavior where pending reloads are auto-applied.
        let prompt_calls = if reload_hook.is_empty() {
            ""
        } else {
            "__devenv_reload_apply\n        __devenv_restore_path"
        };

        let config_fish_content = format!(
            r#"# devenv fish init

# Restore devenv PATH after user config may have modified it.
# _DEVENV_PATH is a colon-separated string from bash; split into fish list.
set -gx PATH (string split ":" -- $_DEVENV_PATH)

# Wrap fish_prompt for devenv reload hooks and prompt prefix.
if functions -q fish_prompt
    functions -c fish_prompt __devenv_user_fish_prompt
    function fish_prompt
        __devenv_user_fish_prompt
        {prompt_calls}
    end
else
    function fish_prompt
        {prompt_calls}
        echo -n "(devenv) > "
    end
end

# Hot-reload hook
{reload_hook}
"#,
            reload_hook = reload_hook,
            prompt_calls = prompt_calls,
        );

        std::fs::write(ctx.init_dir.join("devenv.fish"), config_fish_content)?;
        Ok(())
    }
}
