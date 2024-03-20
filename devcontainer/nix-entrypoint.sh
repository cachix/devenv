#!/bin/bash
# Attempt to start daemon
set +e 
if ! pidof nix-daemon > /dev/null 2>&1; then
    start_ok=false
    if [ "$(id -u)" = "0" ]; then
        # shellcheck disable=SC1091
        # shellcheck source=/dev/null
        ( . /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh; /nix/var/nix/profiles/default/bin/nix-daemon > /tmp/nix-daemon.log 2>&1 ) &
        # shellcheck disable=SC2181
        if [ "$?" = "0" ]; then
            start_ok=true
        fi
    elif type sudo > /dev/null 2>&1; then
        sudo -n sh -c '. /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh; /nix/var/nix/profiles/default/bin/nix-daemon > /tmp/nix-daemon.log 2>&1' &
        # shellcheck disable=SC2181
        if [ "$?" = "0" ]; then
            start_ok=true
        fi
    fi
    if [ "${start_ok}" = "false" ]; then
            echo -e 'Failed to start nix-daemon as root. Set multiUser to false in your feature configuraiton if you would\nprefer to run the container as a non-root. You may also start the daemon manually if you have sudo\ninstalled and configured for your user by running "sudo -c nix-daemon &"'
    fi
fi
sudo setfacl --remove-default /tmp
exec "$@"