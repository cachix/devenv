# devenv hook for nushell
# Usage: Add to your config.nu:
#   source (devenv hook nu | save --force ~/.cache/devenv/hook.nu; "~/.cache/devenv/hook.nu")
# Or: devenv hook nu | save --force ~/.cache/devenv/hook.nu
#     source ~/.cache/devenv/hook.nu

$env._DEVENV_HOOK_UNTRUSTED = ""

# Shared activation logic. Returns silently when no project exists or the
# user is already inside a devenv shell.
def --env _devenv_activate [show_stderr: bool] {
    if ("DEVENV_ROOT" in $env) {
        return
    }

    let result = (^devenv hook-should-activate | complete)

    if $show_stderr and ($result.stderr | str trim) != "" {
        print -e $result.stderr
    }

    if $result.exit_code == 0 {
        let dir = ($result.stdout | str trim)
        if $dir != "" {
            do { cd $dir; ^devenv shell }
            $env._DEVENV_HOOK_UNTRUSTED = ""
            # If the devenv shell exited due to cd outside the project, follow the user there
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
    ($env.config | get -o hooks.env_change.PWD | default []) | append {||
        # Inside devenv shell: exit when leaving the project directory
        if ("DEVENV_ROOT" in $env) {
            if not ($env.PWD == $env.DEVENV_ROOT or ($env.PWD | str starts-with ($env.DEVENV_ROOT + "/"))) {
                # Save target directory so the parent shell can cd there after exit
                $env.PWD | save --force ($env.DEVENV_ROOT + "/.devenv/exit-dir")
                exit
            }
            return
        }

        _devenv_activate true
    }
))

# Retry activation on each prompt for untrusted directories (after 'devenv allow')
$env.config = ($env.config | upsert hooks.pre_prompt (
    ($env.config | get -o hooks.pre_prompt | default []) | append {||
        let untrusted = ($env | get -o _DEVENV_HOOK_UNTRUSTED | default "")
        if $untrusted == "" {
            return
        }
        if ("DEVENV_ROOT" in $env) {
            return
        }

        _devenv_activate false
    }
))

# Re-trigger activation after `devenv allow` succeeds, so a failed shell
# startup (e.g. secretspec auth error) can be retried by re-running
# `devenv allow` once the underlying issue is fixed, without having to
# cd out and back in.
def --env devenv [...args] {
    ^devenv ...$args
    let devenv_status = $env.LAST_EXIT_CODE
    let first_arg = ($args | get -o 0 | default "")
    if $devenv_status == 0 and $first_arg == "allow" and ("DEVENV_ROOT" not-in $env) {
        _devenv_activate false
    }
}
