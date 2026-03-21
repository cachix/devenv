#!/usr/bin/env bash
set -ex

wait_for_port $RUSTFS_PORT
curl -sf "http://127.0.0.1:$RUSTFS_PORT/health"
