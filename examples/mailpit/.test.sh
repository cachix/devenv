#!/usr/bin/env bash
set -ex

devenv up &
DEVENV_PID=$!
export DEVENV_PID

function stop() {
    pkill -P "$DEVENV_PID"
}

trap stop EXIT

timeout 20 bash -c 'until echo > /dev/tcp/localhost/1025; do sleep 0.5; done'

sendmail john@example.com <<EOF
Subject: Hello

Hello world!
EOF
