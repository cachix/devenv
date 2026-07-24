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

# `_DEVENV_HOOK_DIR` marks the one shell process the hook itself spawned.
# Capture it into a non-exported variable, then erase the exported copy so
# it cannot leak into further descendants (a new tmux/zellij pane, a
# manually started nested shell, ...) started from this shell later on —
# those would otherwise inherit it, wrongly conclude they too are
# hook-spawned, and `exit` on cd-out with nothing around to catch them.
if set -q _DEVENV_HOOK_DIR
    set -g _devenv_hook_dir $_DEVENV_HOOK_DIR
    set -e _DEVENV_HOOK_DIR
end

# `builtin cd` that still records fish's own directory history (`$dirprev`,
# used by `cd -`/`prevd`/`nextd`) — replicates what fish's bundled `cd`
# function does around `builtin cd`, without calling `cd` itself. Plain `cd`
# would invoke whatever the user (or a plugin like `zoxide --cmd=cd`)
# overrode it to, which is exactly what `builtin cd` was introduced to avoid
# (see the "infinite loop detected" comment below) — but that also skipped
# fish's own history bookkeeping, which lives in the `cd` function, not a
# variable-change hook, so `cd -` after following the user out here would
# silently skip over the project directory (#2853).
function _devenv_builtin_cd_with_history
    set -l previous $PWD
    builtin cd $argv
    set -l cd_status $status
    if test $cd_status -eq 0 -a "$PWD" != "$previous"
        # 25 matches fish's own MAX_DIR_HIST in share/functions/cd.fish.
        set -l max_dir_hist 25
        set -q dirprev; or set -l dirprev
        set -q dirprev[$max_dir_hist]; and set -e dirprev[1]
        set -U -q dirprev; and set -U -a dirprev $previous; or set -g -a dirprev $previous
        set -U -q dirnext; and set -U -e dirnext; or set -e dirnext
        set -U -q __fish_cd_direction; and set -U __fish_cd_direction prev; or set -g __fish_cd_direction prev
    end
    return $cd_status
end

# Spawn devenv shell in $project_dir and follow the user if they cd'd out.
function _devenv_hook_activate
    set -l project_dir $argv[1]
    # Something else (direnv loading a `.envrc` with `use devenv`, a manually
    # entered devenv shell, ...) may already have activated an environment.
    # Don't stack a redundant devenv shell on top of it.
    if test -n "$DEVENV_ROOT"
        return
    end
    env -C $project_dir _DEVENV_HOOK_DIR=$project_dir _DEVENV_CALLER=hook devenv shell
    # If the devenv shell exited due to cd outside the project, follow the user there
    set -l exit_dir_file "$project_dir/.devenv/exit-dir"
    if test -f "$exit_dir_file"
        set -l target_dir (cat "$exit_dir_file")
        rm -f "$exit_dir_file"
        if test -d "$target_dir"
            # `builtin cd`, not `cd`: avoids "zoxide: infinite loop detected"
            # when the user overrides `cd` (e.g. `zoxide init --cmd=cd`).
            _devenv_builtin_cd_with_history "$target_dir"
        end
    end
end

# Runs on every prompt (after fish has entered its main loop, so the spawned
# devenv shell inherits the real terminal rather than the `| source` pipe).
function _devenv_hook --on-event fish_prompt
    # `DEVENV_ROOT` set means a devenv shell is already active — hook does
    # nothing. Hook-spawned shells (marked by `_devenv_hook_dir`) additionally
    # `exit` when cd-ing outside the project so the parent shell can follow.
    if test -n "$DEVENV_ROOT"
        if set -q _devenv_hook_dir
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
