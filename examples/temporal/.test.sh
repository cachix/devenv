#!/usr/bin/env bash
set -x

echo "Starting temporal service..."
devenv up &
DEVENV_PID=$!
export DEVENV_PID

devenv_stop() {
    pkill -P "$DEVENV_PID"
}

trap devenv_stop EXIT

export TEMPORAL_ADDRESS=127.0.0.1:17233

timeout 20 bash -c 'until echo > /dev/tcp/localhost/17233; do sleep 0.5; done'

sleep 1

if ! temporal operator cluster health; then
	echo "Temporal not started"
	exit 1
fi

echo "Checking namespace..."
temporal operator namespace describe mynamespace

# Print the captured output when temporal status succeeds
echo "Startup complete..."
temporal operator cluster system
echo "$TEMPORAL_OUTPUT"