#!/usr/bin/env bash
set -x

# vault status and store its exit status
check_vault_status() {
  echo "Waiting for service to become available..."
  VAULT_OUTPUT=$(vault status 2>&1)
  VAULT_EXIT_STATUS=$?
}

# Continuously check vault status until it returns successfully (up to a maximum of 100 times)
# shellcheck disable=SC2034
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

# Exit the script
exit "$VAULT_EXIT_STATUS"
