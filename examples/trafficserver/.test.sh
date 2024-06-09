#!/usr/bin/env bash
set -euxo pipefail

# This might help if Traffic Server crashes early
onExit() {
  local logdir f
  logdir="$(traffic_layout info --json | jq -r .LOGDIR)"
  for f in "$logdir"/*; do
    cat "$f"
  done
}

trap onExit EXIT

wait_for_port 8080
curl -vf --max-time 60 http://localhost:8080/nocache/32
