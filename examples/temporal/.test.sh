#!/bin/sh
set -x

# Start the services in the background and store the PID
echo "Starting temporal service..."
devenv up &
DEVENV_PID=$!

# temporal status and store its exit status
check_temporal_status() {
	echo "Waiting for service to become available..."
	TEMPORAL_OUTPUT=$(temporal operator cluster health)
	TEMPORAL_EXIT_STATUS=$?
}

# Continuously check temporal status until it returns successfully (up to a maximum of 20 times)
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

# Clean up by terminating all spawned processes
pkill -P $DEVENV_PID
wait $DEVENV_PID&>/dev/null

# Exit the script
exit $TEMPORAL_EXIT_STATUS
