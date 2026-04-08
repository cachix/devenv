# devenv hook shared function for bash/zsh

_DEVENV_HOOK_PWD=""
_DEVENV_HOOK_LAST_PROJECT=""
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

    # Already showed untrusted message for this directory; silently retry
    if [[ "$_DEVENV_HOOK_UNTRUSTED" == "$PWD" ]]; then
        local project_dir
        project_dir=$(devenv hook-should-activate --last "${_DEVENV_HOOK_LAST_PROJECT:-}" 2>/dev/null)
        if [[ $? -eq 0 && -n "$project_dir" ]]; then
            _DEVENV_HOOK_PWD="$PWD"
            _DEVENV_HOOK_UNTRUSTED=""
            (cd "$project_dir" && devenv shell)
            _DEVENV_HOOK_LAST_PROJECT="$project_dir"
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
        fi
        return $previous_exit_status
    fi

    local project_dir exit_code
    project_dir=$(devenv hook-should-activate --last "${_DEVENV_HOOK_LAST_PROJECT:-}")
    exit_code=$?

    if [[ $exit_code -eq 0 && -n "$project_dir" ]]; then
        _DEVENV_HOOK_PWD="$PWD"
        _DEVENV_HOOK_UNTRUSTED=""
        (cd "$project_dir" && devenv shell)
        _DEVENV_HOOK_LAST_PROJECT="$project_dir"
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
    elif [[ $exit_code -eq 0 ]]; then
        # No project or already activated; cache to avoid rechecking
        _DEVENV_HOOK_PWD="$PWD"
        _DEVENV_HOOK_UNTRUSTED=""
        _DEVENV_HOOK_LAST_PROJECT=""
    else
        # Untrusted project; message already printed to stderr, suppress on retry
        _DEVENV_HOOK_UNTRUSTED="$PWD"
        _DEVENV_HOOK_LAST_PROJECT=""
    fi
    return $previous_exit_status
}
