# devenv hook shared function for bash/zsh

_DEVENV_HOOK_PWD=""
_DEVENV_HOOK_LAST_PROJECT=""

_devenv_hook() {
    local previous_exit_status=$?
    [[ "$_DEVENV_HOOK_PWD" == "$PWD" ]] && return $previous_exit_status
    _DEVENV_HOOK_PWD="$PWD"
    [[ -n "${DEVENV_ROOT:-}" ]] && return $previous_exit_status

    local project_dir
    project_dir=$(devenv hook-should-activate --last "${_DEVENV_HOOK_LAST_PROJECT:-}" 2>/dev/null) || true

    if [[ -n "$project_dir" ]]; then
        (cd "$project_dir" && devenv shell)
        _DEVENV_HOOK_LAST_PROJECT="$project_dir"
    else
        _DEVENV_HOOK_LAST_PROJECT=""
    fi
    return $previous_exit_status
}
