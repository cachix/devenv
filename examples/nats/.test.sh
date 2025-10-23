#!/usr/bin/env bash
set -ex

# Wait for NATS to be ready via monitoring endpoint
timeout 20 bash -c 'until curl -sf http://nats-user:nats-pass@127.0.0.1:8222/healthz >/dev/null 2>&1; do sleep 0.5; done'

# Test: Verify server is responding with auth
curl -f http://nats-user:nats-pass@127.0.0.1:8222/varz | grep -q '"server_name"'

# Test: Verify JetStream is enabled
curl -f http://nats-user:nats-pass@127.0.0.1:8222/jsz | grep -q '"config"'

echo "NATS server is healthy with JetStream and authorization enabled!"
