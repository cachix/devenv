# Appended to posix.sh at build time (devenv/build.rs) to produce hook-bash.sh.

# Register hook
if [[ -z "${PROMPT_COMMAND:-}" ]]; then
    PROMPT_COMMAND="_devenv_hook"
else
    PROMPT_COMMAND="_devenv_hook;${PROMPT_COMMAND}"
fi
