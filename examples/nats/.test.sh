#!/usr/bin/env bash
set -ex

. "$DEVENV_TEST_LIB"

# Wait for NATS to be ready via monitoring endpoint
wait_until 20 curl -sf -o /dev/null "http://nats-user:nats-pass@127.0.0.1:$NATS_MONITORING_PORT/healthz"

# Test: Verify server is responding with auth
curl -f http://nats-user:nats-pass@127.0.0.1:$NATS_MONITORING_PORT/varz | grep -q '"server_name"'

# Test: Verify JetStream is enabled
curl -f http://nats-user:nats-pass@127.0.0.1:$NATS_MONITORING_PORT/jsz | grep -q '"config"'

echo "NATS server is healthy with JetStream and authorization enabled!"
