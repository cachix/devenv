#!/usr/bin/env bash
set -euo pipefail

# The integration harness runs this script directly. If the nested
# `devenv test` incorrectly reaches its test phase, return successfully so
# the outer invocation exposes the false-positive exit code.
recursion_marker=.process-task-failure-running
if test -f "$recursion_marker"; then
  echo "UNEXPECTED_TEST_PHASE_RAN"
  exit 0
fi

touch "$recursion_marker"
trap 'rm -f "$recursion_marker"' EXIT

set +e
output=$(devenv test --no-tui 2>&1)
status=$?
set -e

printf '%s\n' "$output"

if test "$status" -eq 0; then
  echo "FAIL: devenv test reported success after a process-dependent task failed"
  exit 1
fi

grep -q "INTENTIONAL_PROCESS_TASK_FAILURE" <<<"$output"
grep -q "Process tasks failed" <<<"$output"

if grep -q "PROCESS_TASK_PREREQUISITE_MISSING" <<<"$output"; then
  echo "FAIL: the process-dependent task ran without its prerequisite"
  exit 1
fi

marker_dir=.devenv/test-state/process-task-failure
if ! test -f "$marker_dir/prerequisite.ran"; then
  echo "FAIL: the process-dependent task's prerequisite was not scheduled"
  exit 1
fi

pid_file="$marker_dir/server.pid"
if test -f "$pid_file" && kill -0 "$(cat "$pid_file")" 2>/dev/null; then
  echo "FAIL: process sibling was left running after task failure"
  exit 1
fi
