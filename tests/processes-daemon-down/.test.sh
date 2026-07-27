#!/usr/bin/env bash

# Test that the native process manager prevents orphaned daemon processes.
#
# Without the fix, a foreground `devenv up` would overwrite the daemon's PID
# file and socket. When the foreground process exited, its Drop impl would
# delete those files, orphaning the daemon and its children.

set -ex

export DEVENV_RUNTIME="$PWD/.runtime"

PORT=18457

wait_for_port() {
  for i in $(seq 1 30); do
    if curl -s -o /dev/null http://127.0.0.1:$PORT/ 2>/dev/null; then
      return 0
    fi
    sleep 1
  done
  return 1
}

port_free() {
  ! curl -s -o /dev/null --connect-timeout 1 http://127.0.0.1:$PORT/ 2>/dev/null
}

wait_for_port_free() {
  for i in $(seq 1 15); do
    if port_free; then
      return 0
    fi
    sleep 1
  done
  return 1
}

wait_for_pid_exit() {
  for _ in $(seq 1 30); do
    if ! kill -0 "$1" 2>/dev/null; then
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
    ps -o pid,ppid,stat,command -u "$(id -u)" | grep -E 'devenv|http.server' >&2 || true
    for file in concurrent-1.txt concurrent-2.txt; do
      if [ -f "$file" ]; then
        echo "==> $file <==" >&2
        cat "$file" >&2 || true
      fi
    done
    for file in .runtime/processes/daemon.log .runtime/processes/logs/*; do
      if [ -f "$file" ]; then
        echo "==> $file <==" >&2
        tail -n 80 "$file" >&2 || true
      fi
    done
  fi
  devenv processes down >/dev/null 2>&1 || true
  wait_for_port_free || status=1
  exit "$status"
}
trap cleanup EXIT INT TERM

# === Test 1: up -d then down cleans up ===
echo "--- Test 1: basic up -d / down ---"
devenv up -d
devenv processes wait
wait_for_port
devenv processes down
wait_for_port_free || { echo "FAIL: port still bound after down"; exit 1; }
echo "PASS: basic up -d / down"

# === Test 2: up -d attaches when a daemon is already running ===
echo "--- Test 2: up -d attaches when a daemon is already running ---"
devenv up -d
devenv processes wait
wait_for_port

# A second `up -d` must attach to the running daemon (start up-enabled processes
# over the control socket) without erroring and without clobbering the daemon's
# PID file / socket.
devenv up -d
devenv processes wait

# Daemon should still be healthy and stoppable with a single `down`.
curl -s -o /dev/null http://127.0.0.1:$PORT/ || { echo "FAIL: daemon died after attaching up"; devenv processes down || true; exit 1; }

# A non-interactive foreground `up` (no -d) against a running daemon must fail
# fast, not attach and block forever. Assert on the message: a hang killed by
# the timeout also exits non-zero, so the exit code alone can't tell a clean
# reject from a hang.
timeout 15 devenv up --no-tui >up_out.txt 2>&1 || true
grep -q "Processes already running" up_out.txt || {
  echo "FAIL: non-interactive foreground up should fail fast when a daemon is running"
  cat up_out.txt
  devenv processes down || true
  exit 1
}

devenv processes down
wait_for_port_free || { echo "FAIL: port still bound after down"; exit 1; }
echo "PASS: up -d attaches when daemon running"

# === Test 3: up -d / down / restart ===
echo "--- Test 3: restart after down ---"
devenv up -d
devenv processes wait
wait_for_port
devenv processes down
wait_for_port_free || { echo "FAIL: port bound before restart"; exit 1; }

devenv up -d
devenv processes wait
wait_for_port || { echo "FAIL: restart failed"; exit 1; }
devenv processes down
sleep 1
port_free || { echo "FAIL: port bound after second down"; exit 1; }
echo "PASS: restart after down"

# === Test 4: double down is safe ===
echo "--- Test 4: double down ---"
devenv up -d
devenv processes wait
devenv processes down
wait_for_port_free || true
# Second down should fail gracefully, not crash
devenv processes down 2>&1 || true
port_free || { echo "FAIL: port bound after double down"; exit 1; }
echo "PASS: double down"

# === Test 5: concurrent cold daemon starts have one owner ===
echo "--- Test 5: concurrent daemon startup ---"
devenv up -d >concurrent-1.txt 2>&1 &
UP_ONE=$!
devenv up -d >concurrent-2.txt 2>&1 &
UP_TWO=$!
wait "$UP_ONE"
wait "$UP_TWO"
devenv processes wait
wait_for_port

DAEMON_PID=$(sed -n '1p' "$DEVENV_RUNTIME/processes/native-manager.pid")
kill -0 "$DAEMON_PID"
DAEMON_PATTERN="daemon-processes $DEVENV_RUNTIME/processes/daemon-config.json"
DAEMON_COUNT=$(
  ps -eo args= |
    grep -F "$DAEMON_PATTERN" |
    grep -v grep |
    wc -l
)
test "$DAEMON_COUNT" -eq 1 || {
  echo "FAIL: expected one native daemon, found $DAEMON_COUNT"
  cat concurrent-1.txt concurrent-2.txt
  exit 1
}

devenv processes down
wait_for_port_free || { echo "FAIL: port bound after concurrent start cleanup"; exit 1; }
wait_for_pid_exit "$DAEMON_PID" || { echo "FAIL: concurrent-start daemon survived down"; exit 1; }
echo "PASS: concurrent daemon startup"

echo "All daemon-down tests passed!"
