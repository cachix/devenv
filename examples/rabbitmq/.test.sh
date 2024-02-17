#!/usr/bin/env bash
set -ex

devenv up &
DEVENV_PID=$!
export DEVENV_PID

devenv_stop() {
    pkill -P "$DEVENV_PID"
}

trap devenv_stop EXIT

timeout 20 bash -c 'until rabbitmqctl -q status 2>/dev/null; do sleep 0.5; done'
