#!/usr/bin/env bash
set -x

export TEMPORAL_ADDRESS=127.0.0.1:17233

timeout 20 bash -c 'until echo > /dev/tcp/localhost/17233; do sleep 0.5; done'

# Continuously check temporal status until it returns successfully (up to a maximum of 20 times)
# shellcheck disable=SC2034
for i in $(seq 1 20); do
	check_temporal_status
	if [ $TEMPORAL_EXIT_STATUS -eq 0 ]; then
		echo "Service is up..."
		break
	else
		sleep 1
	fi
done

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
exit "$TEMPORAL_EXIT_STATUS"