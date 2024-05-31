#!/usr/bin/env bash
set -euxo pipefail

wait_for_port 8080
curl -vf --max-time 60 http://localhost:8080/nocache/32
