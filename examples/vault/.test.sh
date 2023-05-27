#!/bin/sh
set -x

# Start the services in the background and store the PID
echo "Starting vault service..."
devenv up&
DEVENV_PID=$!

# vault status and store its exit status
check_vault_status() {
  echo "Waiting for service to become available..."
  VAULT_OUTPUT=$(vault status 2>&1)
  VAULT_EXIT_STATUS=$?
}

# Continuously check vault status until it returns successfully (up to a maximum of 100 times)
for i in $(seq 1 20); do
  check_vault_status
  if [ $VAULT_EXIT_STATUS -eq 0 ]; then
    echo "Service is up..."
    break
  else
    sleep 1
  fi
done

# Print the captured output when vault status succeeds
echo "Startup complete..."
vault version
echo "$VAULT_OUTPUT"

# Clean up by terminating all spawned processes
pkill -P $DEVENV_PID
wait $DEVENV_PID &> /dev/null

# Exit the script
exit $VAULT_EXIT_STATUS