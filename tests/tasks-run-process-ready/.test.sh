#!/usr/bin/env bash
set -euo pipefail

# Regression test for https://github.com/cachix/devenv/issues/3030
#
# `devenv tasks run` built its task config without resolving the bash path, so
# exec readiness probes were spawned via an empty program name and failed with
# ENOENT on every attempt. The process started but never reached Ready, so any
# task gated on it via @ready waited forever.
#
# The timeout is a liveness guard, not a timing assertion: the regression makes
# this command hang indefinitely, and without it the suite would block rather
# than report a failure.

state=".devenv/state"
rm -f "$state/after-ready-ran" "$state/probe-target-ready"

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

echo "PASS: task depending on a process readiness probe ran under devenv tasks run"
