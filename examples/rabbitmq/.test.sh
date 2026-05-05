#!/usr/bin/env bash
set -ex

timeout 30 bash -c 'until rabbitmq-diagnostics -q check_running 2>/dev/null; do sleep 0.5; done'

# Confirm management plugin booted and serves the API.
timeout 20 bash -c 'until curl -fsS -u guest:guest "http://127.0.0.1:${RABBITMQ_MANAGEMENT_PORT}/api/overview" >/dev/null; do sleep 0.5; done'

rabbitmq-plugins list -q -e | grep -q rabbitmq_management
