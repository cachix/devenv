#!/usr/bin/env bash
set -ex

. "$DEVENV_TEST_LIB"

echo $PGHOST

wait_until 20 psql -c "SELECT 1" mydb