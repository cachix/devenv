#!/usr/bin/env bash
set -ex

. "$DEVENV_TEST_LIB"

endpoint="http://localhost:${OTEL_HEALTH_PORT:?}/"

wait_until 60 curl -sf -o /dev/null "$endpoint"
curl -s "$endpoint" | grep "Server"
