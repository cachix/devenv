#!/usr/bin/env bash
set -ex

timeout 20 bash -c 'until rabbitmqctl -q status 2>/dev/null; do sleep 0.5; done'