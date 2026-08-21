#!/usr/bin/env bash
set -euo pipefail

# End-to-end coverage for issues #3030, #3102, and #2037.
# Timeouts bound deadlocks; PID checks detect leaked forked children.

state=".devenv/state"
rm -f \
  "$state/after-ready-ran" \
  "$state/blocked-backend-started" \
  "$state/chain-backend.pid" \
  "$state/chain-backend-started" \
  "$state/failing-bridge-ran" \
  "$state/failure-source.pid" \
  "$state/failure-source-ready" \
  "$state/probe-target.pid" \
  "$state/probe-target-ready" \
  "$state/probe-target-started" \
  "$state/success-ordering-violation" \
  "$state/unrelated-started"

assert_stopped() {
  pid_file=$1
  label=$2

  if [ ! -f "$pid_file" ]; then
    echo "FAIL: $label never recorded its pid"
    exit 1
  fi

  pid=$(cat "$pid_file")
  # Allow init to reap a killed orphan before treating its PID as live.
  for _ in $(seq 1 50); do
    if ! kill -0 "$pid" 2>/dev/null; then
      return 0
    fi
    sleep 0.1
  done

  ps -o pid,ppid,pgid,stat,etime,command -p "$pid" >&2 || true
  kill -9 "$pid" 2>/dev/null || true
  echo "FAIL: $label process $pid outlived devenv tasks run"
  exit 1
}

status=0
timeout 120 devenv tasks run devenv:processes:chain-backend || status=$?

if [ "$status" -eq 124 ]; then
  echo "FAIL: mixed process/task chain hung"
  exit 1
fi

if [ "$status" -ne 0 ]; then
  echo "FAIL: successful mixed process/task chain exited with $status"
  exit 1
fi

if [ ! -f "$state/probe-target-started" ]; then
  echo "FAIL: source process never started"
  exit 1
fi

if [ ! -f "$state/probe-target-ready" ]; then
  echo "FAIL: source process never became ready"
  exit 1
fi

if [ ! -f "$state/after-ready-ran" ]; then
  echo "FAIL: intermediary task did not run"
  exit 1
fi

if [ ! -f "$state/chain-backend-started" ]; then
  echo "FAIL: downstream process did not start"
  exit 1
fi

if [ -f "$state/success-ordering-violation" ]; then
  echo "FAIL: a mixed-chain node ran before its dependency"
  exit 1
fi

if [ -f "$state/unrelated-started" ]; then
  echo "FAIL: an unrelated process ran outside the requested root closure"
  exit 1
fi

assert_stopped "$state/probe-target.pid" "source"
assert_stopped "$state/chain-backend.pid" "downstream"

failure_status=0
timeout 120 devenv tasks run devenv:processes:blocked-backend \
  >failure-output.txt 2>&1 || failure_status=$?

if [ "$failure_status" -eq 124 ]; then
  echo "FAIL: failing mixed process/task chain hung"
  cat failure-output.txt
  exit 1
fi

if [ "$failure_status" -eq 0 ]; then
  echo "FAIL: failed intermediary task did not fail the command"
  cat failure-output.txt
  exit 1
fi

if [ ! -f "$state/failure-source-ready" ]; then
  echo "FAIL: failure source never became ready"
  cat failure-output.txt
  exit 1
fi

if [ ! -f "$state/failing-bridge-ran" ]; then
  echo "FAIL: failing intermediary task never ran"
  cat failure-output.txt
  exit 1
fi

if [ -f "$state/blocked-backend-started" ]; then
  echo "FAIL: downstream process launched after its task dependency failed"
  cat failure-output.txt
  exit 1
fi

assert_stopped "$state/failure-source.pid" "failure source"

echo "PASS: mixed process/task chains preserve ordering, failure propagation, root selection, and cleanup"
