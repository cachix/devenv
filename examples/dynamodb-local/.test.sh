#!/usr/bin/env bash
set -ex

devenv up &
DEVENV_PID=$!
export DEVENV_PID

function stop() {
    pkill -P "$DEVENV_PID"
}

trap stop EXIT

export AWS_DEFAULT_REGION=fakeRegion
export AWS_ACCESS_KEY_ID=fakeMyKeyId
export AWS_SECRET_ACCESS_KEY=fakeSecretAccessKey

timeout 60 bash -c 'until echo > /dev/tcp/localhost/8000; do sleep 0.5; done'

aws dynamodb list-tables --endpoint-url http://localhost:8000
