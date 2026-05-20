# devenv hook for fish
# Usage: devenv hook fish | source

set -q _DEVENV_HOOK_UNTRUSTED; or set -g _DEVENV_HOOK_UNTRUSTED ""
set -q _DEVENV_ACTIVATE_DIR; or set -g _DEVENV_ACTIVATE_DIR ""

function _devenv_hook --on-variable PWD
    # `DEVENV_ROOT` set means a devenv shell is already active — hook does
    # nothing. Hook-spawned shells (marked by `_DEVENV_HOOK_DIR`) additionally
    # `exit` when cd-ing outside the project so the parent shell can follow.
    if test -n "$DEVENV_ROOT"
        if test -n "$_DEVENV_HOOK_DIR"
            switch $PWD
                case "$DEVENV_ROOT" "$DEVENV_ROOT/*"
                case '*'
                    printf '%s' $PWD > "$DEVENV_ROOT/.devenv/exit-dir"
                    exit
            end
        end
        return
    end

    # Suppress stderr when retrying the same untrusted PWD (the "not allowed"
    # message was already shown the first time round).
    set -l project_dir
    if test "$_DEVENV_HOOK_UNTRUSTED" = "$PWD"
        set project_dir (devenv hook-should-activate 2>/dev/null)
    else
        set project_dir (devenv hook-should-activate)
    end
    set -l exit_code $status

    if test $exit_code -eq 0 -a -n "$project_dir"
        # Signal to _devenv_hook_prompt to activate on the next prompt rather
        # than spawning here. Spawning inside a PWD event handler means the
        # subprocess inherits whatever in-progress shell state exists at that
        # moment (e.g. zoxide's __zoxide_loop recursion guard), which leaks
        # into the devenv shell and breaks tools that set such sentinels.
        set -g _DEVENV_ACTIVATE_DIR $project_dir
        set -g _DEVENV_HOOK_UNTRUSTED ""
    else if test $exit_code -ne 0
        set -g _DEVENV_HOOK_UNTRUSTED $PWD
    else
        set -g _DEVENV_HOOK_UNTRUSTED ""
    end
end

# Spawn devenv shell in $project_dir and follow the user if they cd'd out.
function _devenv_hook_activate
    set -l project_dir $argv[1]
    set -l exit_dir_file "$project_dir/.devenv/exit-dir"
    # Drop stale state from an earlier failed cleanup before launching a new shell.
    command rm -f "$exit_dir_file"
    env -C $project_dir _DEVENV_HOOK_DIR=$project_dir devenv shell
    # If the devenv shell exited due to cd outside the project, follow the user there
    if test -f "$exit_dir_file"
        set -l target_dir (cat "$exit_dir_file")
        command rm -f "$exit_dir_file"
        if test -d "$target_dir"
            builtin cd "$target_dir"
        end
    end
end

function _devenv_hook_prompt --on-event fish_prompt
    if test -n "$_DEVENV_ACTIVATE_DIR"
        set -l project_dir $_DEVENV_ACTIVATE_DIR
        set -g _DEVENV_ACTIVATE_DIR ""
        _devenv_hook_activate $project_dir
    else if test -n "$_DEVENV_HOOK_UNTRUSTED"
        # Retry activation after `devenv allow`; activates immediately if now trusted.
        _devenv_hook
        if test -n "$_DEVENV_ACTIVATE_DIR"
            set -l project_dir $_DEVENV_ACTIVATE_DIR
            set -g _DEVENV_ACTIVATE_DIR ""
            _devenv_hook_activate $project_dir
        end
    end
end

# Trigger initial check on the first prompt rather than inline.
#
# The hook is typically loaded via `devenv hook fish | source`, which makes
# `source`'s stdin a pipe. `devenv shell` spawned inline would inherit that
# closed pipe, detect no tty, and exit immediately on EOF.
#
# Deferring to fish_prompt runs the initial check after fish has entered
# its main loop, where stdin is the real terminal.
function _devenv_hook_init --on-event fish_prompt
    functions -e _devenv_hook_init
    _devenv_hook
    if test -n "$_DEVENV_ACTIVATE_DIR"
        set -l project_dir $_DEVENV_ACTIVATE_DIR
        set -g _DEVENV_ACTIVATE_DIR ""
        _devenv_hook_activate $project_dir
    end
end
