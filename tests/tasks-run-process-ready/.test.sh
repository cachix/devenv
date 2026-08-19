#!/usr/bin/env bash
set -euo pipefail

# Regression test for two `devenv tasks run` bugs on the same code path.
#
# https://github.com/cachix/devenv/issues/3030
#
# `devenv tasks run` built its task config without resolving the bash path, so
# exec readiness probes were spawned via an empty program name and failed with
# ENOENT on every attempt. The process started but never reached Ready, so any
# task gated on it via @ready waited forever.
#
# The timeout is a liveness guard, not a timing assertion: the regression makes
# this command hang indefinitely, and without it the suite would block rather
# than report a failure.
#
# https://github.com/cachix/devenv/issues/3102
#
# A process pulled into the graph as a dependency was never stopped once the
# graph finished. Processes are spawned in their own session, so they survived
# the CLI's exit and were reparented to init, holding their ports and data
# directories. The pid check below fails if that comes back.

state=".devenv/state"
rm -f "$state/after-ready-ran" "$state/probe-target-ready" "$state/probe-target.pid"

status=0
timeout 120 devenv tasks run test:after-ready || status=$?

if [ "$status" -eq 124 ]; then
  echo "FAIL: devenv tasks run hung waiting on the readiness probe"
  exit 1
fi

if [ "$status" -ne 0 ]; then
  echo "FAIL: devenv tasks run exited with $status"
  exit 1
fi

if [ ! -f "$state/probe-target-ready" ]; then
  echo "FAIL: the process never started"
  exit 1
fi

if [ ! -f "$state/after-ready-ran" ]; then
  echo "FAIL: task gated on the readiness probe did not run"
  exit 1
fi

if [ ! -f "$state/probe-target.pid" ]; then
  echo "FAIL: the process never recorded its pid"
  exit 1
fi

# stop_all() awaits each process's teardown before `devenv tasks run` returns,
# so a live pid here means the process was orphaned, not that it is still
# winding down.
pid=$(cat "$state/probe-target.pid")
if kill -0 "$pid" 2>/dev/null; then
  kill -9 "$pid" 2>/dev/null || true
  echo "FAIL: process $pid outlived devenv tasks run"
  exit 1
fi

echo "PASS: task depending on a process readiness probe ran, and the process was stopped"
