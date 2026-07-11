# Appended to posix.sh at build time (devenv/build.rs) to produce hook-zsh.sh.

# Tell posix.sh which shell to spawn `devenv shell` as. Without this, devenv
# falls back to `$SHELL` (the login shell), which is frequently stale and can
# disagree with the shell this hook was actually loaded into.
_devenv_hook_shell_dialect=zsh

# Register hook via precmd
typeset -ag precmd_functions
if (( ! ${precmd_functions[(I)_devenv_hook]} )); then
    precmd_functions=(_devenv_hook $precmd_functions)
fi
