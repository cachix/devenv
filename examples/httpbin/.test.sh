#!/usr/bin/env bash
set -euxo pipefail

for port in 8080 8081; do
  wait_for_port "$port"
  curl -vf "http://127.0.0.1:$port/headers"
done
