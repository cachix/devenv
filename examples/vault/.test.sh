#!/usr/bin/env bash
set -ex

# vault status and store its exit status
check_vault_status() {
  echo "Waiting for service to become available..."
  VAULT_OUTPUT=$(vault status 2>&1)
  VAULT_EXIT_STATUS=$?
}

trap devenv_stop EXIT

timeout 20 bash -c 'until echo > /dev/tcp/localhost/8200; do sleep 0.5; done'

# Exit the script
exit $VAULT_EXIT_STATUS