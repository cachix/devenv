# devenv hook for nushell
# Usage: Add to your config.nu:
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
                # Only signal the outer hook when it provided a nonce; a
                # bare `devenv shell` has no parent waiting to read this.
                if ("_DEVENV_EXIT_NONCE" in $env) and $env._DEVENV_EXIT_NONCE != "" {
                    ($env._DEVENV_EXIT_NONCE + "\n" + $env.PWD) | save --force ($env.DEVENV_ROOT + "/.devenv/exit-dir")
                }
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
            # Per-session nonce: a stale .devenv/exit-dir from any prior
            # session (e.g. left behind because a user-defined `rm` swallowed
            # the cleanup) will not match and is ignored.
            let nonce = $"(random chars --length 32)"
            with-env { _DEVENV_HOOK_DIR: $dir, _DEVENV_EXIT_NONCE: $nonce } {
                do { cd $dir; ^devenv shell }
            }
            $env._DEVENV_HOOK_UNTRUSTED = ""
            let exit_dir_file = ($dir + "/.devenv/exit-dir")
            if ($exit_dir_file | path exists) {
                # Decode leniently: a stale or corrupt exit-dir with non-UTF-8
                # bytes must be ignored, not abort the hook with an error.
                let content = (open --raw $exit_dir_file | decode utf-8)
                rm -f $exit_dir_file
                # Match the `<nonce>\n` prefix rather than slicing the nonce out:
                # `str substring`'s range end is exclusive on older nushell and
                # inclusive on newer, so a closed-end slice would drop the last
                # nonce character on some versions and never match. `str
                # starts-with` plus an open-ended slice behave the same across
                # versions.
                let prefix = ($nonce + "\n")
                if ($content | str starts-with $prefix) {
                    let target_dir = ($content | str substring ($prefix | str length)..)
                    if ($target_dir | path exists) {
                        cd $target_dir
                    }
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
