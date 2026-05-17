#!/usr/bin/env bash
set -ex

wait_for_port 9324 60

curl -sf http://localhost:9324/health

QUEUE_URL=$(curl -s "http://localhost:9324/?Action=ListQueues" | grep -o '<QueueUrl>[^<]*</QueueUrl>' | sed 's/<[^>]*>//g')

if [[ "$QUEUE_URL" != *"/test-queue" ]]; then
  echo "The queue is not created"
  exit 1
fi
