# Appended to posix.sh at build time (devenv/build.rs) to produce hook-bash.sh.

# Register hook
if [[ ${BLE_VERSION-} ]] && ((_ble_version >= 400)); then
    # ble.sh owns PROMPT_COMMAND once attached, so register through its own
    # hook instead. The hook spawns a shell and prints hints, so tell ble.sh
    # the prompt layout changes and it must leave the editing layout first.
    bleopt prompt_command_changes_layout=1
    blehook PRECMD!=_devenv_hook
elif [[ ";${PROMPT_COMMAND[*]:-};" != *";_devenv_hook;"* ]]; then
    if [[ $(declare -p PROMPT_COMMAND 2>/dev/null) == "declare -a"* ]]; then
        PROMPT_COMMAND=(_devenv_hook "${PROMPT_COMMAND[@]}")
    else
        PROMPT_COMMAND="_devenv_hook${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
    fi
fi
