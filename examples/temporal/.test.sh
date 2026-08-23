#!/bin/sh
set -ex

. "$DEVENV_TEST_LIB"

export TEMPORAL_ADDRESS=127.0.0.1:$TEMPORAL_PORT

wait_until 20 temporal operator cluster health

temporal operator namespace describe -n mynamespace
temporal operator cluster system
