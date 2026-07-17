# devenv hook shared function for bash/zsh.

_DEVENV_HOOK_PWD=""
_DEVENV_HOOK_UNTRUSTED=""
# Set once; cleared on the first `_devenv_hook` call (which runs via
# PROMPT_COMMAND/precmd before the shell's very first prompt is ever shown).
_devenv_hook_first_prompt=1

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

# Resolve symlinks with builtins only (no `realpath` dependency): `$PWD`
# preserves symlinks a user navigated through (e.g. macOS's `/tmp` ->
# `/private/tmp`) while `$DEVENV_ROOT` is canonicalized, so comparing the
# raw strings can spuriously conclude the user left the project when they
# never did. Falls back to the raw value if resolution fails (e.g. the
# directory was removed out from under the shell).
_devenv_resolve_path() {
    (cd -P -- "$1" 2>/dev/null && pwd) || printf '%s' "$1"
}

_devenv_hook() {
    local previous_exit_status=$?
    # This shell never showed the user its own prompt before now (e.g. a
    # fresh terminal opened straight into a trusted project) iff this is the
    # first-ever call. If so and the devenv shell we're about to spawn later
    # exits outright (not a cd-out), there is no prior state in this shell
    # worth preserving — propagate the exit so one exit/Ctrl-D closes the
    # whole terminal. Any later activation necessarily follows the user
    # already having used this shell at least once, so it never propagates.
    local is_first_prompt="${_devenv_hook_first_prompt:-}"
    _devenv_hook_first_prompt=""
    [[ "$_DEVENV_HOOK_PWD" == "$PWD" ]] && return $previous_exit_status

    # `DEVENV_ROOT` set means a devenv shell is already active — hook does
    # nothing. Hook-spawned shells (marked by `_devenv_hook_dir`) additionally
    # `exit` when cd-ing outside the project so the parent shell can follow.
    if [[ -n "${DEVENV_ROOT:-}" ]]; then
        if [[ -n "${_devenv_hook_dir:-}" ]]; then
            local resolved_root
            resolved_root=$(_devenv_resolve_path "$DEVENV_ROOT")
            case "$(_devenv_resolve_path "$PWD")" in
                "$resolved_root"|"$resolved_root"/*) ;;
                *)
                    printf '%s' "$PWD" > "${DEVENV_ROOT}/.devenv/exit-dir"
                    exit $previous_exit_status
                    ;;
            esac
        fi
        _DEVENV_HOOK_PWD="$PWD"
        return $previous_exit_status
    fi

    # Suppress stderr when retrying the same untrusted PWD (message was already shown)
    local project_dir exit_code
    if [[ "$_DEVENV_HOOK_UNTRUSTED" == "$PWD" ]]; then
        project_dir=$(devenv hook-should-activate 2>/dev/null)
    else
        project_dir=$(devenv hook-should-activate)
    fi
    exit_code=$?

    if [[ $exit_code -eq 0 && -n "$project_dir" ]]; then
        _DEVENV_HOOK_UNTRUSTED=""
        # Cache PWD before launching so a SIGINT/failure inside devenv shell
        # doesn't leave us re-launching on every prompt redraw.
        _DEVENV_HOOK_PWD="$PWD"
        (
            _devenv_prev_pwd="$OLDPWD"
            cd "$project_dir" && _DEVENV_HOOK_DIR="$project_dir" _DEVENV_PREV_PWD="$_devenv_prev_pwd" _DEVENV_CALLER=hook devenv shell --shell "$_devenv_hook_shell_dialect"
        )
        local exit_dir_file="$project_dir/.devenv/exit-dir"
        if [[ -f "$exit_dir_file" ]]; then
            local target_dir
            target_dir=$(cat "$exit_dir_file")
            rm -f "$exit_dir_file"
            if [[ -d "$target_dir" ]]; then
                cd "$target_dir"
                # Clear rather than set to "$PWD": the very next call needs
                # to actually re-check this new location (it could be a
                # trusted sibling project), not treat it as already-known.
                # Left stale at project_dir, re-entering that *same*
                # project right after leaving it would coincidentally
                # match $PWD again and silently skip reactivation.
                _DEVENV_HOOK_PWD=""
            fi
        elif [[ -n "$is_first_prompt" ]]; then
            exit $previous_exit_status
        fi
    elif [[ $exit_code -eq 0 ]]; then
        # No project; cache to avoid rechecking
        _DEVENV_HOOK_PWD="$PWD"
        _DEVENV_HOOK_UNTRUSTED=""
    else
        # Untrusted project; message already printed to stderr, suppress on retry
        _DEVENV_HOOK_UNTRUSTED="$PWD"
    fi
    return $previous_exit_status
}
