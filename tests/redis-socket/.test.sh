#!/usr/bin/env bash
set -ex

. "$DEVENV_TEST_LIB"

wait_until 20 redis-cli -s "$REDIS_UNIX_SOCKET" ping
