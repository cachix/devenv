#!/usr/bin/env bash
set -ex

echo "Starting vault service..."
devenv up &
DEVENV_PID=$!
export DEVENV_PID

# shellcheck disable=SC2317 # ShellCheck may incorrectly believe that code is unreachable if it's invoked by variable name or in a trap
devenv_stop() {
    pkill -P "$DEVENV_PID"
}

trap devenv_stop EXIT

timeout 20 bash -c 'until echo > /dev/tcp/localhost/8200; do sleep 0.5; done'

timeout 5 bash -c 'until vault status; do sleep 0.5; done'

vault status