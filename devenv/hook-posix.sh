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
            *) exit $previous_exit_status ;;
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
