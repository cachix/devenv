#!/usr/bin/env bash

set -euo pipefail

. "$DEVENV_TEST_LIB"

PORT=18458
stopped=0

restart_if_stopped() {
  if [[ "$stopped" == 1 ]]; then
    devenv up --detach >/dev/null 2>&1 || true
  fi
}
trap restart_if_stopped EXIT

devenv processes wait --timeout 60

list_output=$(devenv processes list)
echo "$list_output"
grep -q '^http[[:space:]]' <<<"$list_output"

devenv processes down
stopped=1
wait_for_http_gone "$PORT" 15 || {
  echo "FAIL: port still bound after down"
  exit 1
}

devenv up --detach
stopped=0

trap - EXIT
