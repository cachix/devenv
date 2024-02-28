#!/usr/bin/env bash
set -ex

timeout 20 bash -c 'until psql -c "SELECT 1" mydb; do sleep 0.5; done'
