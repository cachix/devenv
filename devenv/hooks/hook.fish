# devenv hook for fish
# Usage: devenv hook fish | source

set -q _DEVENV_HOOK_UNTRUSTED; or set -g _DEVENV_HOOK_UNTRUSTED ""

function _devenv_hook --on-variable PWD
    # Inside a hook-spawned devenv shell: exit when leaving the project
    # directory. `_DEVENV_HOOK_DIR` is the marker — set only in shells we
    # spawned below. `DEVENV_ROOT` alone is not enough: direnv (and other
    # tools) may export it into the user's outer shell, and an unguarded
    # `exit` there closes the terminal.
    if set -q _DEVENV_HOOK_DIR; and set -q DEVENV_ROOT
        switch $PWD
            case "$DEVENV_ROOT" "$DEVENV_ROOT/*"
                return
            case '*'
                # Save target directory so the parent shell can cd there after exit
                printf '%s' $PWD > "$DEVENV_ROOT/.devenv/exit-dir"
                exit
        end
    end

    # stderr flows through so user sees the "not allowed" message
    set -l project_dir (devenv hook-should-activate)
    set -l exit_code $status

    if test $exit_code -eq 0 -a -n "$project_dir"
        set -lx _DEVENV_HOOK_DIR $project_dir
        fish --no-config -c 'cd -- $_DEVENV_HOOK_DIR; and devenv shell'
        set -g _DEVENV_HOOK_UNTRUSTED ""
        # If the devenv shell exited due to cd outside the project, follow the user there
        set -l exit_dir_file "$project_dir/.devenv/exit-dir"
        if test -f "$exit_dir_file"
            set -l target_dir (cat "$exit_dir_file")
            rm -f "$exit_dir_file"
            if test -d "$target_dir"
                builtin cd "$target_dir"
            end
        end
    else if test $exit_code -ne 0
        # Untrusted; retry silently on each prompt until allowed
        set -g _DEVENV_HOOK_UNTRUSTED $PWD
    else
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

    set -l project_dir (devenv hook-should-activate 2>/dev/null)
    if test $status -eq 0 -a -n "$project_dir"
        set -lx _DEVENV_HOOK_DIR $project_dir
        fish --no-config -c 'cd -- $_DEVENV_HOOK_DIR; and devenv shell'
        set -g _DEVENV_HOOK_UNTRUSTED ""
        # If the devenv shell exited due to cd outside the project, follow the user there
        set -l exit_dir_file "$project_dir/.devenv/exit-dir"
        if test -f "$exit_dir_file"
            set -l target_dir (cat "$exit_dir_file")
            rm -f "$exit_dir_file"
            if test -d "$target_dir"
                builtin cd "$target_dir"
            end
        end
    end
end

# Trigger initial check on the first prompt rather than inline.
#
# The hook is typically loaded via `devenv hook fish | source`, which makes
# `source`'s stdin a pipe. Any child shell spawned inline (i.e. during the
# `source` itself) inherits that closed pipe as stdin, so `devenv shell`
# detects no tty, disables the watcher UI, and the interactive fish it
# execs into exits immediately on EOF.
#
# Deferring to fish_prompt runs the initial check after fish has entered
# its main loop, where stdin is the real terminal.
function _devenv_hook_init --on-event fish_prompt
    functions -e _devenv_hook_init
    _devenv_hook
end
