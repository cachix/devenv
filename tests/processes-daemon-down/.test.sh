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

# === Test 2: foreground up rejects when daemon running ===
echo "--- Test 2: foreground up rejects when daemon running ---"
devenv up -d
devenv processes wait
wait_for_port

# Try foreground up — should fail with "already running"
# Use timeout so we don't wait for crash-looping processes if the guard is missing.
if timeout 10 devenv up --no-tui 2>&1; then
  echo "FAIL: foreground up should have been rejected"
  devenv processes down || true
  exit 1
fi
echo "Foreground up correctly rejected"

# Daemon should still be healthy
curl -s -o /dev/null http://127.0.0.1:$PORT/ || { echo "FAIL: daemon died"; exit 1; }
devenv processes down
sleep 1
port_free || { echo "FAIL: port still bound"; exit 1; }
echo "PASS: foreground up rejects when daemon running"

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
