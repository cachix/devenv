#!/usr/bin/env bash
set -ex

timeout 20 bash -c 'until redis-cli -s $REDIS_UNIX_SOCKET ping 2>/dev/null; do sleep 0.5; done'
