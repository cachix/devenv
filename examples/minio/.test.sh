#!/usr/bin/env bash
set -ex

devenv up &
DEVENV_PID=$!
export DEVENV_PID

devenv_stop() {
    pkill -P "$DEVENV_PID"
}

trap devenv_stop EXIT

timeout 20 bash -c 'until echo > /dev/tcp/localhost/9000; do sleep 0.5; done'

mc admin info local
