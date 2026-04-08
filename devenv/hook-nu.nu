# devenv hook for nushell
# Usage: Add to your config.nu:
#   source (devenv hook nu | save --force ~/.cache/devenv/hook.nu; "~/.cache/devenv/hook.nu")
# Or: devenv hook nu | save --force ~/.cache/devenv/hook.nu
#     source ~/.cache/devenv/hook.nu

$env._DEVENV_HOOK_UNTRUSTED = ""

$env.config = ($env.config | upsert hooks.env_change.PWD (
    ($env.config | get -i hooks.env_change.PWD | default []) | append {||
        # Inside devenv shell: exit when leaving the project directory
        if ("DEVENV_ROOT" in $env) {
            if not ($env.PWD == $env.DEVENV_ROOT or ($env.PWD | str starts-with ($env.DEVENV_ROOT + "/"))) {
                # Save target directory so the parent shell can cd there after exit
                $env.PWD | save --force ($env.DEVENV_ROOT + "/.devenv/exit-dir")
                exit
            }
            return
        }

        let last = ($env | get -i _DEVENV_HOOK_LAST_PROJECT | default "")
        let result = (^devenv hook-should-activate --last $last | complete)

        if ($result.stderr | str trim) != "" {
            print -e $result.stderr
        }

        if $result.exit_code == 0 {
            let dir = ($result.stdout | str trim)
            if $dir != "" {
                do { cd $dir; ^devenv shell }
                $env._DEVENV_HOOK_LAST_PROJECT = $dir
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
                $env._DEVENV_HOOK_LAST_PROJECT = ""
                $env._DEVENV_HOOK_UNTRUSTED = ""
            }
        } else {
            $env._DEVENV_HOOK_LAST_PROJECT = ""
            $env._DEVENV_HOOK_UNTRUSTED = $env.PWD
        }
    }
))

# Retry activation on each prompt for untrusted directories (after 'devenv allow')
$env.config = ($env.config | upsert hooks.pre_prompt (
    ($env.config | get -i hooks.pre_prompt | default []) | append {||
        let untrusted = ($env | get -i _DEVENV_HOOK_UNTRUSTED | default "")
        if $untrusted == "" {
            return
        }
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
            }
        }
    }
))
