#!/usr/bin/env bash
set -ex

export AWS_DEFAULT_REGION=fakeRegion
export AWS_ACCESS_KEY_ID=fakeMyKeyId
export AWS_SECRET_ACCESS_KEY=fakeSecretAccessKey

wait_for_port 8000

aws dynamodb list-tables --endpoint-url http://localhost:8000
