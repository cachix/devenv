# Appended to posix.sh at build time (devenv/build.rs) to produce hook-zsh.sh.

# Register hook via precmd
typeset -ag precmd_functions
if (( ! ${precmd_functions[(I)_devenv_hook]} )); then
    precmd_functions=(_devenv_hook $precmd_functions)
fi
