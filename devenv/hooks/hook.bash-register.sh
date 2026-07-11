# Appended to posix.sh at build time (devenv/build.rs) to produce hook-bash.sh.

# Tell posix.sh which shell to spawn `devenv shell` as. Without this, devenv
# falls back to `$SHELL` (the login shell), which is frequently stale and can
# disagree with the shell this hook was actually loaded into.
_devenv_hook_shell_dialect=bash

# Register hook
if [[ -z "${PROMPT_COMMAND:-}" ]]; then
    PROMPT_COMMAND="_devenv_hook"
else
    PROMPT_COMMAND="_devenv_hook;${PROMPT_COMMAND}"
fi
