#!/usr/bin/env bash

# Verify `devenv up` attaches to a running native manager instead of failing.
#
#  - `devenv up -d` starts alpha + beta.
#  - stopping beta leaves alpha running.
#  - a second `devenv up -d` attaches (does not error) and restarts the
#    up-enabled beta over the control socket.

set -ex

PORT_A=18561
PORT_B=18562

reachable() {
  curl -s -o /dev/null --connect-timeout 1 "http://127.0.0.1:$1/" 2>/dev/null
}

wait_for_port() {
  for _ in $(seq 1 30); do
    if reachable "$1"; then return 0; fi
    sleep 1
  done
  return 1
}

wait_for_port_free() {
  for _ in $(seq 1 15); do
    if ! reachable "$1"; then return 0; fi
    sleep 1
  done
  return 1
}

# Start both processes.
devenv up -d
devenv processes wait
wait_for_port "$PORT_A" || { echo "FAIL: alpha did not start"; devenv processes down || true; exit 1; }
wait_for_port "$PORT_B" || { echo "FAIL: beta did not start"; devenv processes down || true; exit 1; }

# Stop beta; alpha stays up.
devenv processes stop beta
wait_for_port_free "$PORT_B" || { echo "FAIL: beta did not stop"; devenv processes down || true; exit 1; }
reachable "$PORT_A" || { echo "FAIL: alpha died after stopping beta"; devenv processes down || true; exit 1; }

# Second up attaches and restarts the up-enabled beta (must not error).
devenv up -d
devenv processes wait
wait_for_port "$PORT_B" || { echo "FAIL: beta not restarted by attaching up"; devenv processes down || true; exit 1; }
reachable "$PORT_A" || { echo "FAIL: alpha died after attaching up"; devenv processes down || true; exit 1; }

# Stop beta; an attaching `devenv up -d beta` must honour the subset and only
# (re)start beta, not alpha (which is already running) and not anything else.
devenv processes stop beta
wait_for_port_free "$PORT_B" || { echo "FAIL: beta did not stop"; devenv processes down || true; exit 1; }
devenv up -d beta
devenv processes wait
wait_for_port "$PORT_B" || { echo "FAIL: beta not restarted by subset attach"; devenv processes down || true; exit 1; }
reachable "$PORT_A" || { echo "FAIL: alpha died after subset attach"; devenv processes down || true; exit 1; }

devenv processes down

# `devenv processes attach` requires a running manager: after down it must fail
# fast with a helpful message instead of hanging or attaching to nothing.
if devenv processes attach >attach_out.txt 2>&1; then
  echo "FAIL: attach should fail when no manager is running"
  exit 1
fi
grep -q "No processes running" attach_out.txt || {
  echo "FAIL: unexpected attach error:"
  cat attach_out.txt
  exit 1
}

echo "All process-up-attach tests passed!"
