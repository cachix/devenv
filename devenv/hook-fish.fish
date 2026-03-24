# devenv hook for fish
# Usage: devenv hook fish | source

set -g _DEVENV_HOOK_LAST_PROJECT ""

function _devenv_hook --on-variable PWD
    if set -q DEVENV_ROOT
        return
    end

    set -l project_dir (devenv hook-should-activate --last "$_DEVENV_HOOK_LAST_PROJECT" 2>/dev/null)
    or return

    if test -n "$project_dir"
        set -lx _DEVENV_HOOK_DIR $project_dir
        fish -c 'cd -- $_DEVENV_HOOK_DIR; and devenv shell'
        set -g _DEVENV_HOOK_LAST_PROJECT $project_dir
    else
        set -g _DEVENV_HOOK_LAST_PROJECT ""
    end
end

# Trigger initial check
if test -n "$PWD"
    _devenv_hook
end
