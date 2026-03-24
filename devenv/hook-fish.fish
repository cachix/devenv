# devenv hook for fish
# Usage: devenv hook fish | source

set -g _DEVENV_HOOK_LAST_PROJECT ""
set -g _DEVENV_HOOK_UNTRUSTED ""

function _devenv_hook --on-variable PWD
    # Inside devenv shell: exit when leaving the project directory
    if set -q DEVENV_ROOT
        switch $PWD
            case "$DEVENV_ROOT" "$DEVENV_ROOT/*"
                return
            case '*'
                exit
        end
    end

    # stderr flows through so user sees the "not allowed" message
    set -l project_dir (devenv hook-should-activate --last "$_DEVENV_HOOK_LAST_PROJECT")
    set -l exit_code $status

    if test $exit_code -eq 0 -a -n "$project_dir"
        set -lx _DEVENV_HOOK_DIR $project_dir
        fish -c 'cd -- $_DEVENV_HOOK_DIR; and devenv shell'
        set -g _DEVENV_HOOK_LAST_PROJECT $project_dir
        set -g _DEVENV_HOOK_UNTRUSTED ""
    else if test $exit_code -ne 0
        # Untrusted; retry silently on each prompt until allowed
        set -g _DEVENV_HOOK_UNTRUSTED $PWD
        set -g _DEVENV_HOOK_LAST_PROJECT ""
    else
        set -g _DEVENV_HOOK_LAST_PROJECT ""
        set -g _DEVENV_HOOK_UNTRUSTED ""
    end
end

function _devenv_hook_prompt --on-event fish_prompt
    # Retry activation for untrusted directories after 'devenv allow'
    if test -z "$_DEVENV_HOOK_UNTRUSTED"
        return
    end
    # Inside devenv shell: no retry needed
    if set -q DEVENV_ROOT
        return
    end

    set -l project_dir (devenv hook-should-activate --last "$_DEVENV_HOOK_LAST_PROJECT" 2>/dev/null)
    if test $status -eq 0 -a -n "$project_dir"
        set -lx _DEVENV_HOOK_DIR $project_dir
        fish -c 'cd -- $_DEVENV_HOOK_DIR; and devenv shell'
        set -g _DEVENV_HOOK_LAST_PROJECT $project_dir
        set -g _DEVENV_HOOK_UNTRUSTED ""
    end
end

# Trigger initial check
if test -n "$PWD"
    _devenv_hook
end
