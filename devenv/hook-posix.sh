# devenv hook shared function for bash/zsh

_DEVENV_HOOK_PWD=""
_DEVENV_HOOK_UNTRUSTED=""

_devenv_hook() {
    local previous_exit_status=$?
    [[ "$_DEVENV_HOOK_PWD" == "$PWD" ]] && return $previous_exit_status

    # Inside devenv shell: exit when leaving the project directory
    if [[ -n "${DEVENV_ROOT:-}" ]]; then
        case "$PWD" in
            "${DEVENV_ROOT}"|"${DEVENV_ROOT}"/*) ;;
            *)
                # Save target directory so the parent shell can cd there after exit
                printf '%s' "$PWD" > "${DEVENV_ROOT}/.devenv/exit-dir"
                exit $previous_exit_status
                ;;
        esac
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
        (cd "$project_dir" && devenv shell)
        # If the devenv shell exited due to cd outside the project, follow the user there
        local exit_dir_file="$project_dir/.devenv/exit-dir"
        if [[ -f "$exit_dir_file" ]]; then
            local target_dir
            target_dir=$(cat "$exit_dir_file")
            rm -f "$exit_dir_file"
            if [[ -d "$target_dir" ]]; then
                cd "$target_dir"
            fi
        fi
        # Cache PWD after any exit-dir cd so the early-return check reflects reality
        _DEVENV_HOOK_PWD="$PWD"
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
