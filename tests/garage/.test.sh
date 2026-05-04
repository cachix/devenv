#!/usr/bin/env bash
set -ex

wait_for_port "$GARAGE_ADMIN_PORT"
wait_for_port "$GARAGE_S3_PORT"

curl -sf -H "Authorization: Bearer devtoken" \
  "http://127.0.0.1:$GARAGE_ADMIN_PORT/v1/health" >/dev/null
