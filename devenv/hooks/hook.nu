# devenv hook for nushell
#
# Loaded automatically (no config.nu edit needed) when devenv is installed via
# Nix, which ships this under $nu.vendor-autoload-dirs. If you're running a
# devenv build that didn't install it there, add it to your own autoload dir:
#   mkdir ($nu.default-config-dir | path join autoload)
#   devenv hook nu | save --force ($nu.default-config-dir | path join autoload/devenv-hook.nu)

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
            # `path expand` resolves symlinks: `$env.PWD` preserves symlinks
            # a user navigated through (e.g. macOS's `/tmp` -> `/private/tmp`)
            # while `$env.DEVENV_ROOT` is canonicalized, so comparing the raw
            # strings can spuriously conclude the user left the project when
            # they never did.
            let resolved_pwd = ($env.PWD | path expand)
            let resolved_root = ($env.DEVENV_ROOT | path expand)
            if not ($resolved_pwd == $resolved_root or ($resolved_pwd | str starts-with ($resolved_root + "/"))) {
                $env.PWD | save --force ($env.DEVENV_ROOT + "/.devenv/exit-dir")
                exit
            }
        }
        return
    }

    let result = (^devenv hook-should-activate | complete)
    let retrying = ($env._DEVENV_HOOK_UNTRUSTED == $env.PWD)
    if not $retrying and ($result.stderr | str trim) != "" {
        print -e $result.stderr
    }

    if $result.exit_code == 0 {
        let dir = ($result.stdout | str trim)
        if $dir != "" {
            # `--shell nu`: without this, devenv falls back to `$SHELL` (the
            # login shell), which is frequently stale and can disagree with
            # the shell this hook was actually loaded into.
            with-env { _DEVENV_HOOK_DIR: $dir, _DEVENV_CALLER: "hook" } { do { cd $dir; ^devenv shell --shell nu } }
            $env._DEVENV_HOOK_UNTRUSTED = ""
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

$env.config = ($env.config | upsert hooks.env_change.PWD (
    ($env.config | get -o hooks.env_change.PWD | default []) | append {|| _devenv_hook }
))

# Retry activation on each prompt for untrusted directories (after `devenv allow`)
$env.config = ($env.config | upsert hooks.pre_prompt (
    ($env.config | get -o hooks.pre_prompt | default []) | append {||
        if $env._DEVENV_HOOK_UNTRUSTED != "" {
            _devenv_hook
        }
    }
))
