#!/usr/bin/env bash
set -ex

wait_for_port "$GARAGE_ADMIN_PORT"
wait_for_port "$GARAGE_S3_PORT"

curl -sf -H "Authorization: Bearer devtoken" \
  "http://127.0.0.1:$GARAGE_ADMIN_PORT/v1/health" >/dev/null

BUCKET_NAME="test-bucket"
export GARAGE_RPC_SECRET="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

# Test if a bucket exists
function assert_bucket_exists() {
  for _ in $(seq 1 20); do
    if ! garage bucket info "$BUCKET_NAME"; then
      sleep 1
      continue
    fi
  done
}

if assert_bucket_exists; then
  echo "Bucket '$BUCKET_NAME' exists"
else
  echo "Bucket '$BUCKET_NAME' does not exist"
  exit 1
fi
