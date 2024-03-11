#!/usr/bin/env bash
set -ex

timeout 60 bash -c 'until echo > /dev/tcp/localhost/9325; do sleep 0.5; done'

QUEUE_NAME=$(curl http://localhost:9325/statistics/queues -s | jq .[].name -r)

if [[ "$QUEUE_NAME" != "test-queue" ]]; then
  echo "The queue is not created"
  exit 1
fi
