#!/usr/bin/env bash
set -ex

. "$DEVENV_TEST_LIB"

endpoint="http://localhost:13133/"

wait_until 60 curl -sf -o /dev/null "$endpoint"
curl -s "$endpoint" | grep "Server"
