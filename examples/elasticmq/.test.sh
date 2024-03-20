#!/usr/bin/env bash
set -ex

wait_for_port 9325 60

QUEUE_NAME=$(curl http://localhost:9325/statistics/queues -s | jq .[].name -r)

if [[ "$QUEUE_NAME" != "test-queue" ]]; then
  echo "The queue is not created"
  exit 1
fi
