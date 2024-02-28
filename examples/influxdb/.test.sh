#!/usr/bin/env bash
set -ex

wait_for_port 8087 60

influx --port 8087 --execute "CREATE DATABASE devenv"
DATABASES=$(influx  --port 8087 --execute "SHOW DATABASES" | grep devenv)

if [[ "$DATABASES" != "devenv" ]]; then
  echo "The influxdb database was not created"
  exit 1
fi
