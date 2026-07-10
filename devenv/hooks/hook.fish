# devenv hook for fish
# Usage: devenv hook fish | source

set -q _DEVENV_HOOK_UNTRUSTED; or set -g _DEVENV_HOOK_UNTRUSTED ""
set -q _DEVENV_ACTIVATE_DIR; or set -g _DEVENV_ACTIVATE_DIR ""

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

function _devenv_hook --on-variable PWD
    # `DEVENV_ROOT` set means a devenv shell is already active — hook does
    # nothing. Hook-spawned shells (marked by `_devenv_hook_dir`) additionally
    # `exit` when cd-ing outside the project so the parent shell can follow.
    if test -n "$DEVENV_ROOT"
        if set -q _devenv_hook_dir
            # `path resolve` (builtin, no `realpath` dependency): `$PWD`
            # preserves symlinks a user navigated through (e.g. macOS's
            # `/tmp` -> `/private/tmp`) while `$DEVENV_ROOT` is canonicalized,
            # so comparing the raw strings can spuriously conclude the user
            # left the project when they never did.
            set -l resolved_root (path resolve $DEVENV_ROOT)
            switch (path resolve $PWD)
                case "$resolved_root" "$resolved_root/*"
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
#
# $argv[2] = "first" marks activation from _devenv_hook_init: this shell
# never showed the user its own prompt before deciding to activate, so it
# exists purely to host this one devenv session (e.g. a fresh terminal tab
# opened straight into a trusted project). If the inner shell then exits
# outright (not a cd-out — no exit-dir file), there is no prior state in
# this outer shell worth preserving, so propagate the exit: one exit/Ctrl-D
# closes the whole terminal, same as a plain shell or direnv would. Any
# later activation (via _devenv_hook_prompt) necessarily happened after the
# user already saw and used this shell at least once, so it never
# propagates — they may want that shell back.
function _devenv_hook_activate
    set -l project_dir $argv[1]
    set -l activation_kind $argv[2]
    # The decision to activate was made on an earlier PWD change and only
    # acted on at this prompt (see the comment on _devenv_hook above). In
    # between, something else (direnv loading a `.envrc` with `use devenv`,
    # a manually entered devenv shell, ...) may have already activated an
    # environment for this directory. Don't stack a redundant devenv shell
    # on top of it.
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
    else if test "$activation_kind" = first
        exit
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
        _devenv_hook_activate $project_dir first
    end
end
