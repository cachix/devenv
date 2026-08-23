#!/usr/bin/env bash
set -ex

. "$DEVENV_TEST_LIB"

wait_until 30 rabbitmq-diagnostics -q check_running

# Confirm management plugin booted and serves the API.
wait_until 20 curl -fsS -o /dev/null -u guest:guest \
  "http://127.0.0.1:${RABBITMQ_MANAGEMENT_PORT}/api/overview"

rabbitmq-plugins list -q -e | grep -q rabbitmq_management
