#!/usr/bin/env bash

# Cross-manager acceptance test for the internal capability contract and the
# detached external-manager lifecycle. Keep manager-specific behavior out of
# this fixture: every manager advertising background_start must satisfy the
# same observable start/down contract.

set -euo pipefail

export DEVENV_NO_AI_AGENT=1

ACTIVE_MANAGER=
ACTIVE_PORT=

reachable() {
  local port=$1
  curl -sf -o /dev/null --connect-timeout 1 "http://127.0.0.1:$port/" 2>/dev/null
}

wait_for_port() {
  local port=$1
  local attempt
  for attempt in $(seq 1 60); do
    if reachable "$port"; then
      return 0
    fi
    sleep 1
  done
  return 1
}

wait_for_port_free() {
  local port=$1
  local attempt
  for attempt in $(seq 1 30); do
    if ! reachable "$port"; then
      return 0
    fi
    sleep 1
  done
  return 1
}

runtime_for() {
  printf '%s/.runtime/%s' "$PWD" "$1"
}

run_devenv() {
  local manager=$1
  shift
  DEVENV_RUNTIME="$(runtime_for "$manager")" \
    devenv --profile "$manager" "$@"
}

dump_diagnostics() {
  local manager=$1
  local runtime
  runtime=$(runtime_for "$manager")

  ps -o pid,ppid,pgid,stat,command -u "$(id -u)" \
    | grep -E 'devenv|process-compose|honcho|hivemind|overmind|mprocs|http.server' >&2 \
    || true

  if [ -f ".devenv/processes.log" ]; then
    tail -n 120 .devenv/processes.log >&2 || true
  fi

  if [ -d "$runtime/processes" ]; then
    for file in "$runtime/processes"/* "$runtime/processes"/logs/*; do
      if [ -f "$file" ]; then
        echo "==> $file <==" >&2
        tail -n 80 "$file" >&2 || true
      fi
    done
  fi
}

cleanup() {
  local status=$?
  trap - EXIT INT TERM

  if [ -n "$ACTIVE_MANAGER" ]; then
    if [ "$status" -ne 0 ]; then
      dump_diagnostics "$ACTIVE_MANAGER"
    fi
    run_devenv "$ACTIVE_MANAGER" processes down >/dev/null 2>&1 || true
    if [ -n "$ACTIVE_PORT" ]; then
      wait_for_port_free "$ACTIVE_PORT" || status=1
    fi
  fi

  exit "$status"
}
trap cleanup EXIT INT TERM

assert_capabilities() {
  local manager=$1
  local expected=$2
  local output

  output=$(run_devenv "$manager" eval process.manager.capabilities)
  jq -e --argjson expected "$expected" \
    '.["process.manager.capabilities"] == $expected' <<<"$output" >/dev/null
}

assert_adapter() {
  local manager=$1
  local expected=$2
  local output

  output=$(run_devenv "$manager" eval process.manager.adapter)
  jq -e --argjson expected "$expected" \
    '.["process.manager.adapter"] == $expected' <<<"$output" >/dev/null
}

assert_detached_lifecycle() {
  local manager=$1
  local port=$2
  local log="${manager}-up.log"
  local capabilities
  local adapter
  local runtime_process_dir
  local state_file
  local pid_file

  echo "Testing detached lifecycle: $manager"
  ACTIVE_MANAGER=$manager
  ACTIVE_PORT=$port

  if ! run_devenv "$manager" up -d >"$log" 2>&1; then
    cat "$log" >&2
    return 1
  fi
  wait_for_port "$port"

  # Reaching the service after `up -d` returned proves it outlived the client.
  reachable "$port"

  pid_file=".devenv/profiles/$manager/processes.pid"
  test -L "$pid_file"
  runtime_process_dir=$(dirname "$(readlink "$pid_file")")
  state_file="$runtime_process_dir/external-manager.json"
  capabilities=$(run_devenv "$manager" eval process.manager.capabilities \
    | jq -c '.["process.manager.capabilities"]')
  adapter=$(run_devenv "$manager" eval process.manager.adapter \
    | jq -c '.["process.manager.adapter"]')
  jq -e \
    --arg manager "$manager" \
    --argjson capabilities "$capabilities" \
    --argjson adapter "$adapter" \
    '.manager_id == $manager
      and .capabilities == $capabilities
      and .adapter == $adapter
      and .capabilities_source == "nix"
      and .adapter_source == "nix"
      and (if $manager == "overmind"
           then (.stop_command | type) == "string"
           else has("stop_command") | not
           end)' \
    "$state_file" >/dev/null

  run_devenv "$manager" processes down
  wait_for_port_free "$port"
  test ! -e "$state_file"

  ACTIVE_MANAGER=
  ACTIVE_PORT=
}

echo "Checking selected capability declarations"
assert_capabilities native \
  '{"background_start":true,"devenv_attach":true,"wait_ready":true,"individual_control":true,"cold_start_subset":true}'
assert_capabilities process-compose \
  '{"background_start":true,"devenv_attach":false,"wait_ready":false,"individual_control":false,"cold_start_subset":true}'
assert_capabilities overmind \
  '{"background_start":true,"devenv_attach":false,"wait_ready":false,"individual_control":false,"cold_start_subset":true}'
assert_capabilities honcho \
  '{"background_start":true,"devenv_attach":false,"wait_ready":false,"individual_control":false,"cold_start_subset":true}'
assert_capabilities hivemind \
  '{"background_start":true,"devenv_attach":false,"wait_ready":false,"individual_control":false,"cold_start_subset":false}'
assert_capabilities mprocs \
  '{"background_start":false,"devenv_attach":false,"wait_ready":false,"individual_control":false,"cold_start_subset":false}'

echo "Checking selected adapter declarations"
assert_adapter native '{"terminal":"none","stop":"native-api","client":"native-api"}'
assert_adapter process-compose '{"terminal":"none","stop":"process-scope","client":"none"}'
assert_adapter overmind '{"terminal":"none","stop":"command","client":"none"}'
assert_adapter honcho '{"terminal":"none","stop":"process-scope","client":"none"}'
assert_adapter hivemind '{"terminal":"none","stop":"process-scope","client":"none"}'
assert_adapter mprocs '{"terminal":"controlling","stop":"process-scope","client":"none"}'

if [ "$(uname -s)" = Darwin ]; then
  echo "Checking mprocs shell uses the native macOS clipboard provider"
  run_devenv mprocs shell -- bash -c \
    'PATH=/usr/bin:/bin; test "$(command -v pbcopy)" = /usr/bin/pbcopy'
fi

assert_detached_lifecycle process-compose 18771
assert_detached_lifecycle honcho 18772
assert_detached_lifecycle hivemind 18773
assert_detached_lifecycle overmind 18774

echo "Checking unsupported detached launch is rejected before spawn: mprocs"
rm -f mprocs-was-invoked mprocs-up.log
if run_devenv mprocs up -d >mprocs-up.log 2>&1; then
  echo "mprocs unexpectedly accepted detached launch" >&2
  exit 1
fi
if [ -e mprocs-was-invoked ]; then
  echo "mprocs executable was invoked before its capability was rejected" >&2
  exit 1
fi
grep -Fq "process manager 'mprocs' does not support background start" mprocs-up.log
if reachable 18775; then
  echo "mprocs started the test service despite rejecting detached launch" >&2
  exit 1
fi

echo "All process-manager capability and lifecycle tests passed"
