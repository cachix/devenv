use super::{InteractiveArgs, RcfileContext, ShellDialect};
use std::path::{Path, PathBuf};

/// Bash shell dialect implementation.
pub struct BashDialect;

impl ShellDialect for BashDialect {
    fn name(&self) -> &str {
        "bash"
    }

    fn interactive_args(&self) -> InteractiveArgs {
        // bash --noprofile --rcfile <path> -i
        //
        // - `--noprofile`: Skip login shell files (/etc/profile, ~/.bash_profile) to avoid PATH overrides
        // - `--rcfile <path>`: Source our custom init script
        // - `-i`: Force interactive mode (must come AFTER --rcfile due to bash argument parsing)
        InteractiveArgs {
            prefix: vec!["--noprofile".into(), "--rcfile".into()],
            suffix: vec!["-i".into()],
        }
    }

    fn rcfile_content(&self, ctx: &RcfileContext) -> String {
        format!(
            r#"# Disable history during init so devenv internal commands don't pollute history.
# The task runner will re-enable it when handing control to the user.
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

# Save PATH before ~/.bashrc potentially modifies it
_DEVENV_PATH="$PATH"

# Source user's bashrc for their customizations (aliases, prompt, etc.)
if [ -e "$HOME/.bashrc" ]; then
    source "$HOME/.bashrc"
fi

# Restore devenv PATH after ~/.bashrc may have modified it
export PATH="$_DEVENV_PATH"
# Note: _DEVENV_PATH is kept set for the reload hook to restore PATH after direnv

# Hot-reload hook (keybinding and PROMPT_COMMAND integration)
{reload_hook}

# Signal that shell initialization is complete (for PTY task runner)
echo "__DEVENV_SHELL_READY__"
"#,
            env_diff_helpers = ctx.env_diff_helpers,
            env_script_path = ctx.env_script_path.to_string_lossy(),
            reload_hook = ctx.reload_hook,
        )
    }

    fn env_diff_helpers(&self) -> &str {
        r#"
# Environment diff helpers (inspired by direnv)
# Diff is stored in _DEVENV_DIFF env var (not a file) so each shell has its own state
# Uses gzip+base64 encoding for compact storage

# Variables to ignore in diff (shell internals that change dynamically)
__devenv_ignored_var() {
    case "$1" in
        _*|PWD|OLDPWD|SHLVL|SHELL|SHELLOPTS|BASHOPTS|BASH_*|HISTCMD|HISTFILE)
            return 0 ;;
        PS1|PS2|PS3|PS4|PROMPT|PROMPT_COMMAND|PROMPT_DIRTRIM)
            return 0 ;;
        COMP_*|READLINE_*|MAILCHECK|COLUMNS|LINES|RANDOM|SECONDS|LINENO|EPOCHSECONDS|EPOCHREALTIME|SRANDOM)
            return 0 ;;
        STARSHIP_*|__fish*|DIRENV_*|nix_saved_*)
            return 0 ;;
        *)
            return 1 ;;
    esac
}

__devenv_capture_env() {
    # Capture exported variables using declare -p for proper escaping
    declare -p -x 2>/dev/null | LC_ALL=C sort
}

__devenv_serialize_diff() {
    # Serialize diff (stdin) to base64-encoded gzip
    gzip -c | base64 -w0
}

__devenv_deserialize_diff() {
    # Deserialize diff from base64-encoded gzip to stdout
    echo "$1" | base64 -d | gzip -d 2>/dev/null
}

__devenv_compute_diff() {
    # Compare before ($1) and current env, return diff via _DEVENV_DIFF env var
    local before_file="$1"

    # Create temp files
    local after_file diff_content
    after_file=$(mktemp)
    diff_content=$(mktemp)
    __devenv_capture_env > "$after_file"

    # Extract var name from declare -p line
    __devenv_parse_var() {
        local line="${1#declare -x }"
        if [[ "$line" == *=* ]]; then
            echo "${line%%=*}"
        else
            echo "$line"
        fi
    }

    # Build associative arrays for before/after
    local -A before_vars after_vars
    while IFS= read -r line; do
        [[ "$line" != declare\ -x\ * ]] && continue
        local var=$(__devenv_parse_var "$line")
        [[ -z "$var" ]] && continue
        __devenv_ignored_var "$var" && continue
        before_vars["$var"]="$line"
    done < "$before_file"

    while IFS= read -r line; do
        [[ "$line" != declare\ -x\ * ]] && continue
        local var=$(__devenv_parse_var "$line")
        [[ -z "$var" ]] && continue
        __devenv_ignored_var "$var" && continue
        after_vars["$var"]="$line"
    done < "$after_file"

    # Find PREV entries (vars that were modified or removed)
    for var in "${!before_vars[@]}"; do
        if [[ "${after_vars[$var]}" != "${before_vars[$var]}" ]]; then
            echo "P:${before_vars[$var]}" >> "$diff_content"
        fi
    done

    # Find NEXT entries (vars that were added or modified)
    for var in "${!after_vars[@]}"; do
        if [[ -z "${before_vars[$var]+x}" ]]; then
            echo "N:$var" >> "$diff_content"
        elif [[ "${after_vars[$var]}" != "${before_vars[$var]}" ]]; then
            echo "N:$var" >> "$diff_content"
        fi
    done

    # Serialize and store in env var
    _DEVENV_DIFF=$(__devenv_serialize_diff < "$diff_content")
    export _DEVENV_DIFF

    rm -f "$after_file" "$diff_content"
}

__devenv_apply_reverse_diff() {
    # Reverse the diff: restore PREV values, unset NEXT-only vars
    [[ -z "$_DEVENV_DIFF" ]] && return

    local -A prev_vars
    local diff_content
    diff_content=$(__devenv_deserialize_diff "$_DEVENV_DIFF")

    # First pass: collect and restore PREV declarations
    while IFS= read -r line; do
        if [[ "$line" == P:declare\ * ]]; then
            local decl="${line#P:}"
            local var="${decl#declare -x }"
            var="${var%%=*}"
            prev_vars["$var"]=1
            # Use export instead of eval'ing the declare statement directly,
            # because declare -x inside a function creates a local variable
            # in bash 5.0+.
            eval "export ${decl#declare -x }" 2>/dev/null
        fi
    done <<< "$diff_content"

    # Second pass: unset NEXT vars that weren't in PREV (added vars)
    while IFS= read -r line; do
        if [[ "$line" == N:* ]]; then
            local var="${line#N:}"
            if [[ -z "${prev_vars[$var]+x}" ]]; then
                unset "$var"
            fi
        fi
    done <<< "$diff_content"
}
"#
    }

    fn reload_hook(&self, reload_file: &Path) -> String {
        format!(
            r#"
__devenv_reload_apply() {{
    # Source new environment if a reload is pending
    if [ -f "{0}" ]; then
        # Reverse previous diff to restore base environment
        __devenv_apply_reverse_diff

        # Capture env before sourcing new devenv
        local before_file
        before_file=$(mktemp)
        __devenv_capture_env > "$before_file"

        # Source new devenv environment
        source "{0}"
        rm -f "{0}"

        # Compute and store new diff (in _DEVENV_DIFF env var)
        __devenv_compute_diff "$before_file"
        rm -f "$before_file"

        # Update saved PATH for the restore hook
        _DEVENV_PATH="$PATH"
    fi
}}

__devenv_restore_path() {{
    # Restore devenv PATH (in case direnv or other tools modified it)
    export PATH="$_DEVENV_PATH"
}}

__devenv_reload_hook() {{
    __devenv_restore_path
}}

if [[ $- == *i* ]] && command -v bind >/dev/null 2>&1; then
    __devenv_reload_keybind="${{DEVENV_RELOAD_KEYBIND:-\\e\\C-r}}"
    bind -x "\"${{__devenv_reload_keybind}}\":__devenv_reload_apply"
fi

# Append hook so it runs AFTER direnv's _direnv_hook (only if not already added)
if [[ "$PROMPT_COMMAND" != *"__devenv_reload_hook"* ]]; then
    PROMPT_COMMAND="${{PROMPT_COMMAND:+$PROMPT_COMMAND;}}__devenv_reload_hook"
fi
"#,
            reload_file.to_string_lossy()
        )
    }

    fn disable_history(&self) -> &str {
        "set +o history"
    }

    fn enable_history(&self) -> &str {
        "set -o history"
    }

    fn user_rcfile(&self) -> Option<PathBuf> {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".bashrc"))
    }

    fn prompt_prefix(&self) -> &str {
        r#"PS1="(devenv) ${PS1:-}"#
    }
}
