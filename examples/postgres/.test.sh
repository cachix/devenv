#!/usr/bin/env bash
set -ex

devenv up &

timeout 20 bash -c 'until psql -h /tmp -c "SELECT 1" mydb 2>/dev/null; do sleep 0.5; done'
