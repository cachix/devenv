use super::{InteractiveArgs, RcfileContext, ShellDialect};
use std::path::{Path, PathBuf};

/// Fish shell dialect implementation.
///
/// Architecture: We always launch bash first to source the devenv environment
/// (which produces bash syntax). The bash rcfile computes the env diff, saves
/// XDG_CONFIG_HOME, sets XDG_CONFIG_HOME to our init directory so fish reads
/// our config.fish, then execs into fish.
/// Our config.fish restores the original XDG_CONFIG_HOME, sources the user's
/// conf.d files and config.fish, then sets up devenv integration.
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

# Compute and store the initial diff in _DEVENV_DIFF env var
__devenv_compute_diff "$_devenv_before_file"
rm -f "$_devenv_before_file"
unset _devenv_before_file

# Save PATH before fish init potentially modifies it
export _DEVENV_PATH="$PATH"

# Save original XDG_CONFIG_HOME so fish init can restore it
if [ -n "$XDG_CONFIG_HOME" ]; then
    export _DEVENV_REAL_XDG_CONFIG_HOME="$XDG_CONFIG_HOME"
fi

# Point XDG_CONFIG_HOME to our init directory containing fish/config.fish
export XDG_CONFIG_HOME="{fish_config_dir_parent}"

# Re-enable history before exec
set -o history

# Exec into fish (resolve via PATH if not absolute, since the devenv
# environment may have added it after this process started)
if [ ! -x "{target_shell}" ] && ! command -v "{target_shell}" >/dev/null 2>&1; then
    echo "devenv: error: shell '{target_shell}' not found" >&2
    echo "devenv: add fish to your devenv.nix packages or set SHELL to an absolute path" >&2
fi
exec "{target_shell}" -i
echo "devenv: error: failed to exec into {target_shell}" >&2
"#,
            env_diff_helpers = ctx.env_diff_helpers,
            env_script_path = ctx.env_script_path.to_string_lossy(),
            // Fish looks for $XDG_CONFIG_HOME/fish/config.fish, so we set
            // XDG_CONFIG_HOME to the parent of our fish/ directory.
            fish_config_dir_parent = ctx.init_dir.to_string_lossy(),
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
            # Source env diff helpers
            {env_diff_helpers}

            # Reverse previous diff
            __devenv_apply_reverse_diff

            # Capture env before sourcing new devenv
            _before=$(mktemp)
            __devenv_capture_env > "$_before"

            # Source new devenv environment
            source "{reload_file}"
            rm -f "{reload_file}"

            # Compute new diff
            __devenv_compute_diff "$_before"
            rm -f "$_before"

            # Output current environment for the fish process to parse
            export -p
        ' 2>/dev/null)

        # Parse bash export -p output (declare -x VAR="value") and apply
        # each variable to the fish environment via set -gx.
        for line in $reload_output
            # Match lines of the form: declare -x VAR="value"
            if string match -qr '^declare -x ([^=]+)="(.*)"$' -- $line
                set -l var (string match -r '^declare -x ([^=]+)="(.*)"$' -- $line)[2]
                set -l val (string match -r '^declare -x ([^=]+)="(.*)"$' -- $line)[3]
                # PATH, MANPATH, CDPATH are list variables in fish;
                # split colon-separated values into proper lists.
                if test "$var" = PATH -o "$var" = MANPATH -o "$var" = CDPATH
                    set -gx $var (string split ":" -- $val)
                else
                    set -gx $var $val
                end
            else if string match -qr '^declare -x ([^=]+)$' -- $line
                # Variable exported without a value
                set -l var (string match -r '^declare -x ([^=]+)$' -- $line)[2]
                set -gx $var ""
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
            env_diff_helpers = super::BashDialect.env_diff_helpers(),
        )
    }

    fn user_rcfile(&self) -> Option<PathBuf> {
        // Fish config lives at $XDG_CONFIG_HOME/fish/config.fish,
        // defaulting to ~/.config/fish/config.fish.
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".config"))
                    .unwrap_or_default()
            });
        Some(config_home.join("fish").join("config.fish"))
    }

    fn prompt_prefix(&self) -> &str {
        // Fish uses functions for prompts. We prepend (devenv) by wrapping
        // fish_prompt in write_init_files, so we return an empty string here.
        ""
    }

    fn write_init_files(&self, ctx: &RcfileContext) -> std::io::Result<()> {
        let fish_dir = ctx.init_dir.join("fish");
        std::fs::create_dir_all(&fish_dir)?;

        let reload_hook = ctx.reload_hook;

        // When reload is enabled, call both reload-apply and path-restore
        // before each prompt. This matches bash's PROMPT_COMMAND and zsh's
        // precmd behavior where pending reloads are auto-applied.
        let pre_prompt_calls = if reload_hook.is_empty() {
            ""
        } else {
            "__devenv_reload_apply\n        __devenv_restore_path"
        };

        let config_fish_content = format!(
            r#"# devenv fish init - restore XDG_CONFIG_HOME and source user's config

# Restore original XDG_CONFIG_HOME and source user's conf.d + config.fish.
# Fish normally sources conf.d/*.fish before config.fish; since we redirected
# XDG_CONFIG_HOME, we need to source the user's conf.d files manually.
if set -q _DEVENV_REAL_XDG_CONFIG_HOME
    set -gx XDG_CONFIG_HOME $_DEVENV_REAL_XDG_CONFIG_HOME
    set -e _DEVENV_REAL_XDG_CONFIG_HOME

    # Source user's conf.d files (plugins, Fisher, etc.)
    for f in $XDG_CONFIG_HOME/fish/conf.d/*.fish
        source $f
    end

    # Source user's config.fish
    if test -f "$XDG_CONFIG_HOME/fish/config.fish"
        source "$XDG_CONFIG_HOME/fish/config.fish"
    end
else
    set -e XDG_CONFIG_HOME

    # Source user's conf.d files from default location
    for f in $HOME/.config/fish/conf.d/*.fish
        source $f
    end

    # Source user's config.fish from default location
    if test -f "$HOME/.config/fish/config.fish"
        source "$HOME/.config/fish/config.fish"
    end
end

# Restore devenv PATH after user config may have modified it.
# _DEVENV_PATH is a colon-separated string from bash; split into fish list.
set -gx PATH (string split ":" -- $_DEVENV_PATH)

# Set devenv prompt prefix by wrapping fish_prompt
if functions -q fish_prompt
    functions -c fish_prompt __devenv_user_fish_prompt
    function fish_prompt
        {pre_prompt_calls}
        echo -n "(devenv) "
        __devenv_user_fish_prompt
    end
else
    function fish_prompt
        {pre_prompt_calls}
        echo -n "(devenv) > "
    end
end

# Hot-reload hook
{reload_hook}
"#,
            reload_hook = reload_hook,
            pre_prompt_calls = pre_prompt_calls,
        );

        std::fs::write(fish_dir.join("config.fish"), config_fish_content)?;
        Ok(())
    }
}
