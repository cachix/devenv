#!/usr/bin/env bash

set -euo pipefail

PORT=18458
stopped=0

restart_if_stopped() {
  if [[ "$stopped" == 1 ]]; then
    devenv up --detach >/dev/null 2>&1 || true
  fi
}
trap restart_if_stopped EXIT

port_free() {
  ! curl -s -o /dev/null --connect-timeout 1 "http://127.0.0.1:$PORT/" 2>/dev/null
}

wait_for_port_free() {
  for _ in $(seq 1 15); do
    if port_free; then
      return 0
    fi
    sleep 1
  done
  return 1
}

devenv processes wait --timeout 60

list_output=$(devenv processes list)
echo "$list_output"
grep -q '^http[[:space:]]' <<<"$list_output"

devenv processes down
stopped=1
wait_for_port_free || {
  echo "FAIL: port still bound after down"
  exit 1
}

devenv up --detach
stopped=0

trap - EXIT
