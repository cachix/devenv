#!/usr/bin/env bash

# Verify an attaching `devenv up` honours dependencies through the daemon's task
# scheduler (not a CLI-side re-derivation):
#
#  - `devenv up -d` starts alpha + gamma + beta (beta after gamma@ready).
#  - stopping gamma + beta leaves alpha running.
#  - attaching `devenv up -d beta` while gamma is stopped must NOT launch beta
#    (its dependency is unmet) and must NOT hang.
#  - attaching `devenv up -d gamma` then makes gamma ready, and beta follows
#    automatically via its dependency.

set -ex

PORT_ALPHA=18581
PORT_BETA=18582
PORT_GAMMA=18583

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

# Start all three; beta comes up once gamma is ready.
devenv up -d
devenv processes wait
wait_for_port "$PORT_ALPHA" || { echo "FAIL: alpha did not start"; devenv processes down || true; exit 1; }
wait_for_port "$PORT_GAMMA" || { echo "FAIL: gamma did not start"; devenv processes down || true; exit 1; }
wait_for_port "$PORT_BETA"  || { echo "FAIL: beta did not start"; devenv processes down || true; exit 1; }

# Stop gamma and beta; alpha stays up.
devenv processes stop beta
devenv processes stop gamma
wait_for_port_free "$PORT_BETA"  || { echo "FAIL: beta did not stop"; devenv processes down || true; exit 1; }
wait_for_port_free "$PORT_GAMMA" || { echo "FAIL: gamma did not stop"; devenv processes down || true; exit 1; }
reachable "$PORT_ALPHA" || { echo "FAIL: alpha died"; devenv processes down || true; exit 1; }

# Attach `up beta` while gamma is stopped: beta's dependency is unmet, so the
# daemon must hold beta as waiting and NOT launch it (must also not hang). This
# is the regression: the old CLI path dropped the out-of-subset gamma edge and
# launched beta immediately.
# Note: do NOT `devenv processes wait` here — beta is meant to stay waiting on
# gamma, so a global wait would block. `up -d beta` returns once beta has been
# scheduled; we then confirm it stays down for the duration of wait_for_port.
devenv up -d beta
if wait_for_port "$PORT_BETA"; then
  echo "FAIL: beta started without its gamma@ready dependency"
  devenv processes down || true
  exit 1
fi
reachable "$PORT_ALPHA" || { echo "FAIL: alpha died after attaching up beta"; devenv processes down || true; exit 1; }
# beta should be registered and held as waiting (not stopped, not running).
devenv processes list | grep -E '^beta\b' | grep -q waiting || {
  echo "FAIL: beta should be waiting on gamma; got:"; devenv processes list | grep -E '^beta\b'
  devenv processes down || true; exit 1;
}

# Attach `up gamma`: gamma becomes ready, satisfying beta's dependency, so beta
# is launched automatically by the daemon's scheduler.
devenv up -d gamma
wait_for_port "$PORT_GAMMA" || { echo "FAIL: gamma not started by attach"; devenv processes down || true; exit 1; }
wait_for_port "$PORT_BETA"  || { echo "FAIL: beta not launched after gamma became ready"; devenv processes down || true; exit 1; }

devenv processes down

echo "All process-up-attach-deps tests passed!"
