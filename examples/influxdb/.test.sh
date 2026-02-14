#!/usr/bin/env bash
set -ex

# Use a test-local config path to avoid conflicts with existing configs
export INFLUX_CONFIGS_PATH="$PWD/.influxdbv2/configs"

wait_for_port 8086 60

influx setup \
  --username devenv \
  --password devenvpass \
  --org devenv-org \
  --bucket devenv-bucket \
  --force

BUCKETS=$(influx bucket list --org devenv-org | grep devenv-bucket)

if [[ -z "$BUCKETS" ]]; then
  echo "The influxdb bucket was not created"
  exit 1
fi
