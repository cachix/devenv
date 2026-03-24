# devenv hook for nushell
# Usage: Add to your config.nu:
#   source (devenv hook nu | save --force ~/.cache/devenv/hook.nu; "~/.cache/devenv/hook.nu")
# Or: devenv hook nu | save --force ~/.cache/devenv/hook.nu
#     source ~/.cache/devenv/hook.nu

$env.config = ($env.config | upsert hooks.env_change.PWD (
    ($env.config | get -i hooks.env_change.PWD | default []) | append {||
        if ("DEVENV_ROOT" in $env) {
            return
        }

        let last = ($env | get -i _DEVENV_HOOK_LAST_PROJECT | default "")
        let result = (^devenv hook-should-activate --last $last | complete)

        if $result.exit_code == 0 {
            let dir = ($result.stdout | str trim)
            if $dir != "" {
                do { cd $dir; ^devenv shell }
                $env._DEVENV_HOOK_LAST_PROJECT = $dir
            } else {
                $env._DEVENV_HOOK_LAST_PROJECT = ""
            }
        }
    }
))
