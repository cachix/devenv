#!/bin/sh
set -ex

devenv up &
DEVENV_PID=$!
trap "pkill -P $DEVENV_PID" EXIT

timeout 20 bash -c 'until echo > /dev/tcp/localhost/9000; do sleep 0.5; done'

mc admin info local
