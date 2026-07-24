# devenv hook for nushell
#
# Loaded automatically (no config.nu edit needed) when devenv is installed via
# Nix, which ships this under $nu.vendor-autoload-dirs. If you're running a
# devenv build that didn't install it there, add it to your own autoload dir:
#   mkdir ($nu.default-config-dir | path join autoload)
#   devenv hook nu | save --force ($nu.default-config-dir | path join autoload/devenv-hook.nu)

# The project dir we last auto-activated. Lets you `exit` a devenv shell back to
# the parent shell without it immediately re-spawning; cleared once you cd
# elsewhere. `devenv hook-should-activate` is cheap (static binary), so apart
# from this guard the hook runs it every prompt — no result caching, so
# `devenv allow`/`revoke` take effect on the next prompt without a re-`cd`.
$env._DEVENV_HOOK_ACTIVATED = ""
# Last directory reported as untrusted, so the "not allowed" hint is shown once
# per entry rather than on every prompt.
$env._DEVENV_HOOK_UNTRUSTED = ""

# `_DEVENV_HOOK_DIR` marks the one shell process the hook itself spawned;
# it gates the cd-out `exit` so externally-set `DEVENV_ROOT` (e.g. via
# direnv) does not close the user's terminal. Capture it into a plain
# variable, then remove it from `$env` so it cannot leak into further
# descendants (a new tmux/zellij pane, a manually started nested
# shell, ...) started from this shell later on — those would otherwise
# inherit it, wrongly conclude they too are hook-spawned, and `exit` on
# cd-out with nothing around to catch them.
let _devenv_hook_dir = ("_DEVENV_HOOK_DIR" in $env)
hide-env -i _DEVENV_HOOK_DIR

def --env _devenv_hook [] {
    if ("DEVENV_ROOT" in $env) {
        if $_devenv_hook_dir {
            if not ($env.PWD == $env.DEVENV_ROOT or ($env.PWD | str starts-with ($env.DEVENV_ROOT + "/"))) {
                $env.PWD | save --force ($env.DEVENV_ROOT + "/.devenv/exit-dir")
                exit
            }
        }
        return
    }

    # Just exited the devenv shell for this dir — don't re-spawn until you leave.
    if ($env._DEVENV_HOOK_ACTIVATED == $env.PWD) {
        return
    }
    $env._DEVENV_HOOK_ACTIVATED = ""

    let result = (^devenv hook-should-activate | complete)
    let retrying = ($env._DEVENV_HOOK_UNTRUSTED == $env.PWD)
    if not $retrying and ($result.stderr | str trim) != "" {
        print -e $result.stderr
    }

    if $result.exit_code == 0 {
        let dir = ($result.stdout | str trim)
        if $dir != "" {
            $env._DEVENV_HOOK_UNTRUSTED = ""
            # Mark activated before launching so exiting the shell doesn't re-launch.
            $env._DEVENV_HOOK_ACTIVATED = $env.PWD
            with-env { _DEVENV_HOOK_DIR: $dir, _DEVENV_CALLER: "hook" } { do { cd $dir; ^devenv shell } }
            let exit_dir_file = ($dir + "/.devenv/exit-dir")
            if ($exit_dir_file | path exists) {
                let target_dir = (open $exit_dir_file | str trim)
                rm -f $exit_dir_file
                if ($target_dir | path exists) {
                    cd $target_dir
                }
            }
        } else {
            $env._DEVENV_HOOK_UNTRUSTED = ""
        }
    } else {
        $env._DEVENV_HOOK_UNTRUSTED = $env.PWD
    }
}

# Run on every prompt. hook-should-activate is cheap, so there's no separate
# env_change/PWD trigger or trust-DB stamp: each prompt re-checks, which makes
# `devenv allow`/`revoke` (and out-of-tree bindings) take effect immediately.
$env.config = ($env.config | upsert hooks.pre_prompt (
    ($env.config | get -o hooks.pre_prompt | default []) | append {|| _devenv_hook }
))
