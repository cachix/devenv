#!/usr/bin/env bash
set -ex

# Blocks until garage's readiness probe passes (which requires the bucket to
# exist) and fails if garage-configure dies, so the checks below can't race.
wait_for_processes

curl -sf -H "Authorization: Bearer devtoken" \
  "http://127.0.0.1:$GARAGE_ADMIN_PORT/v1/health" >/dev/null

BUCKET_NAME="test-bucket"

if garage bucket info "$BUCKET_NAME" &>/dev/null; then
  echo "Bucket '$BUCKET_NAME' exists"
else
  echo "Bucket '$BUCKET_NAME' does not exist"
  exit 1
fi
