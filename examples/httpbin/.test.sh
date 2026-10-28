#!/usr/bin/env bash
set -euxo pipefail

: "${HTTPBIN_PORTS:?HTTPBIN_PORTS is not set}"

for port in $HTTPBIN_PORTS; do
  wait_for_port "$port"
  curl -vf "http://127.0.0.1:$port/headers"
done
