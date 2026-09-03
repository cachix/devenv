#!/usr/bin/env bash
set -x

# openbao status and store its exit status
check_openbao_status() {
  echo "Waiting for service to become available..."
  OPENBAO_OUTPUT=$(bao status 2>&1)
  OPENBAO_EXIT_STATUS=$?
}

# Continuously check openbao status until it returns successfully (up to a maximum of 100 times)
# shellcheck disable=SC2034
for i in $(seq 1 20); do
  check_openbao_status
  if [ $OPENBAO_EXIT_STATUS -eq 0 ]; then
    echo "Service is up..."
    break
  else
    sleep 1
  fi
done

# Print the captured output when openbao status succeeds
echo "Startup complete..."
bao version
echo "$OPENBAO_OUTPUT"

# Exit the script
exit "$OPENBAO_EXIT_STATUS"
