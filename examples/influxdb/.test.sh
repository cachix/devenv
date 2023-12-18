#!/usr/bin/env bash
set -ex

devenv up &
DEVENV_PID=$!
export DEVENV_PID

function stop() {
    pkill -P "$DEVENV_PID"
}

trap stop EXIT

# We test for the none-default port, configured in the nix file
timeout 10 bash -c 'until echo > /dev/tcp/localhost/8087; do sleep 0.5; done'

influx --execute "CREATE DATABASE devenv"
DATABASES=$(influx --execute "SHOW DATABASES" | grep devenv)

if [[ "$DATABASES" != "devenv" ]]; then
  echo "The influxdb database was not created"
  exit 1
fi