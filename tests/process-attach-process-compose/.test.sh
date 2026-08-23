#!/usr/bin/env bash

# Unsupported operations must fail without disturbing process-compose.

set -eux

export DEVENV_NO_AI_AGENT=1

PORT=18661
PID_FILE="$PWD/.devenv/processes.pid"
STATE_FILE=

manager_pid() {
  jq -r '.scope.leader.pid' "$STATE_FILE"
}

reachable() {
  curl -sf -o /dev/null --connect-timeout 1 "http://127.0.0.1:$PORT/" 2>/dev/null
}

wait_for_port() {
  for _ in $(seq 1 60); do
    if reachable; then
      return 0
    fi
    sleep 1
  done
  return 1
}

wait_for_port_free() {
  for _ in $(seq 1 30); do
    if ! reachable; then
      return 0
    fi
    sleep 1
  done
  return 1
}

cleanup() {
  status=$?
  trap - EXIT INT TERM
  if [ "$status" -ne 0 ]; then
    ps -o pid,ppid,stat,command -u "$(id -u)" | grep -E 'devenv|process-compose|http.server' >&2 || true
    if [ -f .devenv/processes.log ]; then
      tail -n 100 .devenv/processes.log >&2 || true
    fi
  fi
  devenv processes down >/dev/null 2>&1 || true
  wait_for_port_free || status=1
  exit "$status"
}
trap cleanup EXIT INT TERM

devenv up -d
wait_for_port
test -L "$PID_FILE"
RUNTIME_PROCESS_DIR=$(dirname "$(readlink "$PID_FILE")")
STATE_FILE="$RUNTIME_PROCESS_DIR/external-manager.json"
test -s "$STATE_FILE"
jq -e '
  .manager_id == "process-compose"
  and .capabilities_source == "nix"
  and .adapter_source == "nix"
  and .capabilities == {
    "background_start": true,
    "devenv_attach": false,
    "wait_ready": false,
    "individual_control": false,
    "cold_start_subset": true
  }
  and .adapter == {
    "terminal": "none",
    "stop": "process-scope",
    "client": "none"
  }
' "$STATE_FILE" >/dev/null
MANAGER_PID=$(manager_pid)
test "$(sed -n '1p' "$PID_FILE")" = "$MANAGER_PID"
kill -0 "$MANAGER_PID"

if devenv processes attach >attach.txt 2>&1; then
  echo "attach unexpectedly accepted process-compose" >&2
  exit 1
fi
grep -Fq "process manager 'process-compose' does not support devenv attach" attach.txt

for command in \
  "list" \
  "status alpha" \
  "logs alpha"
do
  if devenv processes $command >native-only.txt 2>&1; then
    echo "native-only command unexpectedly accepted process-compose: $command" >&2
    exit 1
  fi
  grep -q "only supported with the native process manager" native-only.txt
  test "$(manager_pid)" = "$MANAGER_PID"
  kill -0 "$MANAGER_PID"
  reachable
done

for command in \
  "start alpha" \
  "stop alpha" \
  "restart alpha"
do
  if devenv processes $command >unsupported-operation.txt 2>&1; then
    echo "unsupported command unexpectedly accepted process-compose: $command" >&2
    exit 1
  fi
  grep -Fq "process manager 'process-compose' does not support individual process control" \
    unsupported-operation.txt
  test "$(manager_pid)" = "$MANAGER_PID"
  kill -0 "$MANAGER_PID"
  reachable
done

if devenv processes wait >wait.txt 2>&1; then
  echo "process-compose wait unexpectedly succeeded" >&2
  exit 1
fi
grep -Fq "process manager 'process-compose' does not support readiness waiting" wait.txt
test "$(manager_pid)" = "$MANAGER_PID"
reachable

devenv processes down
wait_for_port_free

echo "All process-compose compatibility tests passed!"
