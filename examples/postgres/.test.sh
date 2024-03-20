#!/usr/bin/env bash
set -ex

echo $PGHOST

timeout 20 bash -c 'until psql -c "SELECT 1" mydb 2>/dev/null; do sleep 0.5; done'