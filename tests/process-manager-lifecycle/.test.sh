#!/usr/bin/env bash

# Cross-manager acceptance test for the internal capability contract and the
# detached external-manager lifecycle. Keep manager-specific behavior out of
# this fixture: every manager advertising background_start must satisfy the
# same observable start/down contract.

set -euo pipefail

. "$DEVENV_TEST_LIB"

export DEVENV_NO_AI_AGENT=1

ACTIVE_MANAGER=
ACTIVE_PORT=
FOREGROUND_PID=

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

  if [ -n "$FOREGROUND_PID" ]; then
    kill "$FOREGROUND_PID" 2>/dev/null || true
  fi

  if [ -n "$ACTIVE_MANAGER" ]; then
    if [ "$status" -ne 0 ]; then
      dump_diagnostics "$ACTIVE_MANAGER"
    fi
    run_devenv "$ACTIVE_MANAGER" processes down >/dev/null 2>&1 || true
    if [ -n "$ACTIVE_PORT" ]; then
      wait_for_http_gone "$ACTIVE_PORT" || status=1
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

assert_lifecycle_cycle() {
  local manager=$1
  local port=$2
  local capabilities=$3
  local adapter=$4
  local cycle=$5
  local log="${manager}-up-${cycle}.log"
  local runtime_process_dir
  local state_file
  local persistent_state_file
  local pid_file

  if ! run_devenv "$manager" up -d >"$log" 2>&1; then
    cat "$log" >&2
    return 1
  fi
  wait_for_http_ready "$port" 60

  # Reaching the service after `up -d` returned proves it outlived the client.
  http_is_ready "$port"

  pid_file=".devenv/profiles/$manager/processes.pid"
  test -L "$pid_file"
  runtime_process_dir=$(dirname "$(readlink "$pid_file")")
  state_file="$runtime_process_dir/external-manager.json"
  persistent_state_file=".devenv/profiles/$manager/external-manager.json"
  test -e "$persistent_state_file"
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
  wait_for_http_gone "$port" 30
  test ! -e "$state_file"
  test ! -e "$persistent_state_file"
  test ! -L "$pid_file"
}

assert_detached_lifecycle() {
  local manager=$1
  local port=$2
  local capabilities
  local adapter

  echo "Testing detached lifecycle: $manager"
  ACTIVE_MANAGER=$manager
  ACTIVE_PORT=$port

  capabilities=$(run_devenv "$manager" eval process.manager.capabilities \
    | jq -c '.["process.manager.capabilities"]')
  adapter=$(run_devenv "$manager" eval process.manager.adapter \
    | jq -c '.["process.manager.adapter"]')

  # Two cycles, not one. `down` returns as soon as the manager accepts the
  # request, so a manager that stops slowly still passes a single cycle. The
  # second start fails if the first one left a control socket or a live process
  # behind.
  assert_lifecycle_cycle "$manager" "$port" "$capabilities" "$adapter" 1
  assert_lifecycle_cycle "$manager" "$port" "$capabilities" "$adapter" 2

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

# A foreground `devenv up` execs the manager in place, so nothing of devenv's
# runs afterwards to record it. Without state published before the exec, a
# second client sees an idle project and starts a rival manager.
assert_foreground_start_is_shared() {
  local manager=$1
  local port=$2
  local pid_file=".devenv/profiles/$manager/processes.pid"
  local state_file

  echo "Testing foreground lifecycle: $manager"
  ACTIVE_MANAGER=$manager
  ACTIVE_PORT=$port

  run_devenv "$manager" up >"${manager}-foreground.log" 2>&1 </dev/null &
  FOREGROUND_PID=$!
  wait_for_http_ready "$port" 60

  test -L "$pid_file"
  state_file="$(dirname "$(readlink "$pid_file")")/external-manager.json"
  test -e "$state_file"
  jq -e --arg manager "$manager" '.manager_id == $manager' "$state_file" >/dev/null

  # A separate client stops the manager it never started.
  run_devenv "$manager" processes down
  wait_for_http_gone "$port" 30
  wait_for_pid_gone "$FOREGROUND_PID" 30
  FOREGROUND_PID=
  test ! -e "$state_file"
  test ! -L "$pid_file"

  ACTIVE_MANAGER=
  ACTIVE_PORT=
}

assert_foreground_start_is_shared honcho 18772

# The OS clears the runtime directory on its own schedule. A second copy of the
# state under `.devenv` keeps the manager reachable when that happens, instead
# of leaving it running with nothing able to name it.
assert_cleared_runtime_dir_still_stops() {
  local manager=$1
  local port=$2
  local pid_file=".devenv/profiles/$manager/processes.pid"
  local persistent_state_file=".devenv/profiles/$manager/external-manager.json"
  local runtime_process_dir

  echo "Testing recovery from a cleared runtime directory: $manager"
  ACTIVE_MANAGER=$manager
  ACTIVE_PORT=$port

  run_devenv "$manager" up -d >"${manager}-cleared-runtime.log" 2>&1
  wait_for_http_ready "$port" 60

  runtime_process_dir=$(dirname "$(readlink "$pid_file")")
  rm -rf "$runtime_process_dir"

  run_devenv "$manager" processes down
  wait_for_http_gone "$port" 30
  test ! -e "$persistent_state_file"

  ACTIVE_MANAGER=
  ACTIVE_PORT=
}

assert_cleared_runtime_dir_still_stops process-compose 18771

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
if http_is_ready 18775; then
  echo "mprocs started the test service despite rejecting detached launch" >&2
  exit 1
fi

echo "All process-manager capability and lifecycle tests passed"
