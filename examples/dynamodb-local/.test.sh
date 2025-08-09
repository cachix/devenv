#!/usr/bin/env bash
set -ex

export AWS_DEFAULT_REGION=fakeRegion
export AWS_ACCESS_KEY_ID=fakeMyKeyId
export AWS_SECRET_ACCESS_KEY=fakeSecretAccessKey

wait_for_processes

aws dynamodb list-tables --endpoint-url http://127.0.0.1:8000 --output text --no-cli-pager

