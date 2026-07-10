# devenv hook for nushell
#
# Loaded automatically (no config.nu edit needed) when devenv is installed via
# Nix, which ships this under $nu.vendor-autoload-dirs. If you're running a
# devenv build that didn't install it there, add it to your own autoload dir:
#   mkdir ($nu.default-config-dir | path join autoload)
#   devenv hook nu | save --force ($nu.default-config-dir | path join autoload/devenv-hook.nu)

$env._DEVENV_HOOK_UNTRUSTED = ""

# `_DEVENV_HOOK_DIR` is set only on shells the hook itself spawned;
# it gates the cd-out `exit` so externally-set `DEVENV_ROOT`
# (e.g. via direnv) does not close the user's terminal.
def --env _devenv_hook [] {
    if ("DEVENV_ROOT" in $env) {
        if ("_DEVENV_HOOK_DIR" in $env) {
            if not ($env.PWD == $env.DEVENV_ROOT or ($env.PWD | str starts-with ($env.DEVENV_ROOT + "/"))) {
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
            with-env { _DEVENV_HOOK_DIR: $dir } { do { cd $dir; ^devenv shell } }
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
