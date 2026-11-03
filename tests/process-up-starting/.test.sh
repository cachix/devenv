#!/usr/bin/env bash

# Test that a foreground `devenv up` publishes its manager while its processes
# are still starting.
#
# Without the fix, the foreground manager wrote its PID file and socket only
# after every process was ready. Until then a second `devenv up` failed with
# "Process tasks failed", and `devenv up -d` spawned a daemon that could not
# claim the processes and was orphaned once the foreground manager published.

set -ex

. "$DEVENV_TEST_LIB"

PROCESS_RUNTIME_DIR="$(devenv_runtime_dir)/processes"
PID_FILE="$PROCESS_RUNTIME_DIR/native-manager.pid"
DAEMON_PATTERN="daemon-processes $PROCESS_RUNTIME_DIR/daemon-config.json"
FG_PID=

daemon_pids() {
  ps -eo pid=,args= | grep -F "$DAEMON_PATTERN" | grep -v grep | awk '{print $1}'
}

cleanup() {
  status=$?
  trap - EXIT INT TERM
  if [ "$status" -ne 0 ]; then
    ps -o pid,ppid,stat,command -u "$(id -u)" | grep -E 'devenv|sleep infinity' >&2 || true
    for file in fg.log up-d.txt up.txt publication-error.txt startup-failure.txt retry.log "$PROCESS_RUNTIME_DIR"/daemon.log; do
      if [ -f "$file" ]; then
        echo "==> $file <==" >&2
        tail -n 80 "$file" >&2 || true
      fi
    done
  fi
  if [ -n "$FG_PID" ]; then
    kill -INT "$FG_PID" 2>/dev/null || true
    wait "$FG_PID" || true
  fi
  for pid in $(daemon_pids); do
    kill "$pid" 2>/dev/null || true
  done
  rmdir "$PROCESS_RUNTIME_DIR/native.sock" "$PID_FILE" 2>/dev/null || true
  exit "$status"
}
trap cleanup EXIT INT TERM

devenv up --no-tui >fg.log 2>&1 &
FG_PID=$!

# `slow` has launched but is not ready, so the foreground up is still starting.
wait_until 120 test -e .devenv/state/slow-started
wait_until 10 test -s "$PID_FILE" || {
  echo "FAIL: starting foreground up did not publish its PID file"
  exit 1
}

# A detached up must refuse to schedule into the foreground session instead
# of spawning a second manager.
if devenv up -d >up-d.txt 2>&1; then
  echo "FAIL: up -d should refuse while a foreground up is starting"
  cat up-d.txt
  exit 1
fi
grep -q "foreground" up-d.txt || {
  echo "FAIL: unexpected up -d error"
  cat up-d.txt
  exit 1
}
test -z "$(daemon_pids)" || {
  echo "FAIL: up -d spawned a daemon next to the foreground manager"
  exit 1
}

# A non-interactive foreground up must report the running manager.
run_bounded 15 devenv up --no-tui >up.txt 2>&1 || true
grep -q "Processes already running" up.txt || {
  echo "FAIL: second foreground up should report the running manager"
  cat up.txt
  exit 1
}

touch .devenv/state/slow-ready
run_bounded 30 devenv processes wait

kill -INT "$FG_PID"
wait "$FG_PID" || true
FG_PID=
wait_for_path_gone "$PID_FILE"

# Publishing can fail while a setup task is still running. Process supervisor
# shutdown alone does not cancel that task, so startup would hang on its join.
touch .devenv/state/block-setup
check_publication_failure() {
  local path=$1 expected_error=$2 status=0 setup_pid
  rm -f .devenv/state/setup-pid
  mkdir "$path"
  run_bounded 30 devenv up --no-tui >publication-error.txt 2>&1 || status=$?
  if [ "$status" -eq 0 ] || [ "$status" -eq 124 ] || [ "$status" -eq 137 ]; then
    echo "FAIL: publication failure should return an error without hanging (status $status)"
    cat publication-error.txt
    exit 1
  fi
  grep -q "$expected_error" publication-error.txt
  if [ -s .devenv/state/setup-pid ]; then
    read -r setup_pid < .devenv/state/setup-pid
    wait_for_pid_gone "$setup_pid"
  fi
  rmdir "$path"
}

check_publication_failure "$PROCESS_RUNTIME_DIR/native.sock" "Failed to bind API socket"
check_publication_failure "$PID_FILE" "Failed to write manager PID"
test ! -e "$PROCESS_RUNTIME_DIR/native.sock"
test ! -e "$PID_FILE"

# Failure after publication must stop a healthy sibling and remove discovery
# state. Open both gates only after seeing the manager and failing task.
rm -f .devenv/state/block-setup .devenv/state/slow-ready .devenv/state/slow-started
touch .devenv/state/fail-startup
devenv up --no-tui >startup-failure.txt 2>&1 &
FG_PID=$!
wait_until 60 test -s "$PID_FILE"
wait_until 30 test -e .devenv/state/failure-task-started
read -r SLOW_PID < .devenv/state/slow-pid
kill -0 "$SLOW_PID"
touch .devenv/state/slow-ready .devenv/state/fail-now
wait_for_path_gone "$PID_FILE"
status=0
wait "$FG_PID" || status=$?
FG_PID=
test "$status" -ne 0
grep -q "Process tasks failed" startup-failure.txt
grep -q "INTENTIONAL_STARTUP_FAILURE" startup-failure.txt
wait_for_pid_gone "$SLOW_PID"
test ! -e "$PROCESS_RUNTIME_DIR/native.sock"

# A fresh startup after all failure cases proves cleanup released both the
# startup lock and process ownership claims.
rm -f .devenv/state/fail-startup .devenv/state/slow-started
devenv up --no-tui >retry.log 2>&1 &
FG_PID=$!
wait_until 60 test -e .devenv/state/slow-started
run_bounded 30 devenv processes wait
read -r SLOW_PID < .devenv/state/slow-pid
kill -INT "$FG_PID"
wait "$FG_PID" || true
FG_PID=
wait_for_path_gone "$PID_FILE"
wait_for_pid_gone "$SLOW_PID"
test ! -e "$PROCESS_RUNTIME_DIR/native.sock"

echo "All process-up-starting tests passed!"
