#!/usr/bin/env bash
set -ex

. "$DEVENV_TEST_LIB"

wait_until 20 vault status

vault version
vault status
