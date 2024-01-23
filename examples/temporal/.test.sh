#!/bin/sh
set -x

export TEMPORAL_ADDRESS=127.0.0.1:17233

# temporal status and store its exit status
check_temporal_status() {
	echo "Waiting for service to become available..."
	TEMPORAL_OUTPUT=$(temporal operator cluster health)
	TEMPORAL_EXIT_STATUS=$?
}

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

echo "Checking namespace..."
temporal operator namespace describe mynamespace

# Print the captured output when temporal status succeeds
echo "Startup complete..."
temporal operator cluster system
echo "$TEMPORAL_OUTPUT"

# Exit the script
exit "$TEMPORAL_EXIT_STATUS"
