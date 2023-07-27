#!/usr/bin/env bash
set -x

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

# Exit the script
exit $TEMPORAL_EXIT_STATUS