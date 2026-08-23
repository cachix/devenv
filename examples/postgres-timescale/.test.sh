#!/usr/bin/env bash
set -ex

. "$DEVENV_TEST_LIB"

wait_until 20 psql -c "SELECT 1" mydb
