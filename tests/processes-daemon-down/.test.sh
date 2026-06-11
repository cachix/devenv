#!/usr/bin/env bash

# Test that the native process manager prevents orphaned daemon processes.
#
# Without the fix, a foreground `devenv up` would overwrite the daemon's PID
# file and socket. When the foreground process exited, its Drop impl would
# delete those files, orphaning the daemon and its children.

set -ex

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

echo "All daemon-down tests passed!"
