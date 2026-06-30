# devenv hook for fish
# Usage: devenv hook fish | source

# The project dir we last auto-activated. Lets you `exit` a devenv shell back to
# the parent shell without it immediately re-spawning; cleared once you cd
# elsewhere. `devenv hook-should-activate` is cheap (static binary), so apart
# from this guard the hook runs it every prompt — no result caching, so
# `devenv allow`/`revoke` take effect on the next prompt without a re-`cd`.
set -q _DEVENV_HOOK_ACTIVATED; or set -g _DEVENV_HOOK_ACTIVATED ""
# Last directory reported as untrusted, so the "not allowed" hint is shown once
# per entry rather than on every prompt.
set -q _DEVENV_HOOK_UNTRUSTED; or set -g _DEVENV_HOOK_UNTRUSTED ""

# Spawn devenv shell in $project_dir and follow the user if they cd'd out.
function _devenv_hook_activate
    set -l project_dir $argv[1]
    env -C $project_dir _DEVENV_HOOK_DIR=$project_dir devenv shell
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

# Runs on every prompt (after fish has entered its main loop, so the spawned
# devenv shell inherits the real terminal rather than the `| source` pipe).
function _devenv_hook --on-event fish_prompt
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

    # Just exited the devenv shell for this dir — don't re-spawn until you leave.
    if test "$_DEVENV_HOOK_ACTIVATED" = "$PWD"
        return
    end
    set -g _DEVENV_HOOK_ACTIVATED ""

    # Suppress stderr when re-checking the same untrusted PWD (hint already shown).
    set -l project_dir
    if test "$_DEVENV_HOOK_UNTRUSTED" = "$PWD"
        set project_dir (devenv hook-should-activate 2>/dev/null)
    else
        set project_dir (devenv hook-should-activate)
    end
    set -l exit_code $status

    if test $exit_code -eq 0 -a -n "$project_dir"
        set -g _DEVENV_HOOK_UNTRUSTED ""
        # Mark activated before launching so exiting the shell doesn't re-launch.
        set -g _DEVENV_HOOK_ACTIVATED "$PWD"
        _devenv_hook_activate $project_dir
    else if test $exit_code -ne 0
        set -g _DEVENV_HOOK_UNTRUSTED "$PWD"
    else
        set -g _DEVENV_HOOK_UNTRUSTED ""
    end
end
