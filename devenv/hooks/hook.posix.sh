# devenv hook shared function for bash/zsh.

# The project dir we last auto-activated. Lets you `exit` a devenv shell back to
# the parent shell without it immediately re-spawning; cleared once you cd
# elsewhere. `devenv hook-should-activate` is cheap (static binary), so apart
# from this guard the hook just runs it every prompt — no result caching, so
# `devenv allow`/`revoke` take effect on the next prompt without a re-`cd`.
_DEVENV_HOOK_ACTIVATED=""
# Last directory reported as untrusted, so the "not allowed" hint is shown once
# per entry rather than on every prompt.
_DEVENV_HOOK_UNTRUSTED=""

# `_DEVENV_HOOK_DIR` marks the one shell process the hook itself spawned.
# Capture it into a non-exported variable, then unset the exported copy so
# it cannot leak into further descendants (a new tmux/zellij pane, a
# manually started nested shell, ...) started from this shell later on —
# those would otherwise inherit it, wrongly conclude they too are
# hook-spawned, and `exit` on cd-out with nothing around to catch them.
if [[ -n "${_DEVENV_HOOK_DIR:-}" ]]; then
    _devenv_hook_dir="$_DEVENV_HOOK_DIR"
    unset _DEVENV_HOOK_DIR
fi

_devenv_hook() {
    local previous_exit_status=$?

    # `DEVENV_ROOT` set means a devenv shell is already active — hook does
    # nothing. Hook-spawned shells (marked by `_devenv_hook_dir`) additionally
    # `exit` when cd-ing outside the project so the parent shell can follow.
    if [[ -n "${DEVENV_ROOT:-}" ]]; then
        if [[ -n "${_devenv_hook_dir:-}" ]]; then
            case "$PWD" in
                "${DEVENV_ROOT}"|"${DEVENV_ROOT}"/*) ;;
                *)
                    printf '%s' "$PWD" > "${DEVENV_ROOT}/.devenv/exit-dir"
                    exit $previous_exit_status
                    ;;
            esac
        fi
        return $previous_exit_status
    fi

    # Just exited the devenv shell for this dir — don't re-spawn until you leave.
    if [[ "$_DEVENV_HOOK_ACTIVATED" == "$PWD" ]]; then
        return $previous_exit_status
    fi
    _DEVENV_HOOK_ACTIVATED=""

    # Suppress stderr when re-checking the same untrusted PWD (hint already shown).
    local project_dir exit_code
    if [[ "$_DEVENV_HOOK_UNTRUSTED" == "$PWD" ]]; then
        project_dir=$(devenv hook-should-activate 2>/dev/null)
    else
        project_dir=$(devenv hook-should-activate)
    fi
    exit_code=$?

    if [[ $exit_code -eq 0 && -n "$project_dir" ]]; then
        _DEVENV_HOOK_UNTRUSTED=""
        # Mark activated before launching so exiting the shell (or a SIGINT/
        # failure inside it) doesn't re-launch on the next prompt redraw.
        _DEVENV_HOOK_ACTIVATED="$PWD"
        (cd "$project_dir" && _DEVENV_HOOK_DIR="$project_dir" _DEVENV_CALLER=hook devenv shell)
        local exit_dir_file="$project_dir/.devenv/exit-dir"
        if [[ -f "$exit_dir_file" ]]; then
            local target_dir
            target_dir=$(cat "$exit_dir_file")
            rm -f "$exit_dir_file"
            if [[ -d "$target_dir" ]]; then
                cd "$target_dir"
            fi
        fi
    elif [[ $exit_code -eq 0 ]]; then
        # No project here.
        _DEVENV_HOOK_UNTRUSTED=""
    else
        # Untrusted project; hint already printed to stderr, suppress on retry.
        _DEVENV_HOOK_UNTRUSTED="$PWD"
    fi
    return $previous_exit_status
}
