#!/usr/bin/env bash

# Verify an attaching `devenv up` honours dependencies through the daemon's task
# scheduler (not a CLI-side re-derivation), and that the client reply is
# truthful:
#
#  - `devenv up -d` starts alpha + gamma + beta (beta after gamma@ready).
#  - stopping gamma + beta leaves alpha running.
#  - attaching `devenv up -d beta` while gamma is stopped reports beta as
#    scheduled, holds it as waiting (its dependency is unmet), and must NOT
#    launch it or hang.
#  - `devenv processes wait` settles while beta is parked on stopped gamma
#    (previously this blocked forever).
#  - `devenv up -d <unknown-name>` fails with a nonzero exit.
#  - attaching `devenv up -d gamma` then makes gamma ready, and beta follows
#    automatically via its dependency.

set -ex

# The assertions below grep info-level messages ("Scheduled: ..."); the
# AI-agent auto-quiet mode would suppress them, so opt out.
export DEVENV_NO_AI_AGENT=1

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

# Attach `up beta` while gamma is stopped: the daemon schedules beta and
# reports so truthfully, but beta's dependency is unmet, so it must be held as
# waiting and NOT launched (and the request must not hang). This is the
# regression: the old CLI path dropped the out-of-subset gamma edge and
# launched beta immediately.
devenv up -d beta >up_beta.txt 2>&1
grep -q "Scheduled: beta" up_beta.txt || {
  echo "FAIL: attach up should report beta as scheduled; got:"
  cat up_beta.txt
  devenv processes down || true; exit 1;
}
# The Up reply is sent only after beta is re-armed as waiting, so its state is
# immediately observable — no polling needed.
devenv processes list | grep -E '^beta\b' | grep -q waiting || {
  echo "FAIL: beta should be waiting on gamma; got:"; devenv processes list | grep -E '^beta\b'
  devenv processes down || true; exit 1;
}
# beta must not have launched: its gamma dependency is stopped.
if reachable "$PORT_BETA"; then
  echo "FAIL: beta started without its gamma@ready dependency"
  devenv processes down || true
  exit 1
fi
reachable "$PORT_ALPHA" || { echo "FAIL: alpha died after attaching up beta"; devenv processes down || true; exit 1; }

# `devenv processes wait` must settle while beta is parked on the stopped
# gamma: only external action can unblock it, so nothing can make further
# startup progress. Previously this hung forever. The timeout is a failure
# bound only, not a timing assertion.
timeout 60 devenv processes wait || {
  echo "FAIL: processes wait did not settle while beta was parked on stopped gamma"
  devenv processes down || true; exit 1;
}

# An unknown name fails loudly with a nonzero exit instead of silently
# succeeding.
if devenv up -d nosuchproc >typo.txt 2>&1; then
  echo "FAIL: up -d with an unknown process name should fail"
  cat typo.txt
  devenv processes down || true
  exit 1
fi
grep -q "not found in configuration" typo.txt || {
  echo "FAIL: unexpected error for unknown process name:"
  cat typo.txt
  devenv processes down || true; exit 1;
}

# Attach `up gamma`: gamma becomes ready, satisfying beta's dependency, so beta
# is launched automatically by the daemon's scheduler.
devenv up -d gamma
wait_for_port "$PORT_GAMMA" || { echo "FAIL: gamma not started by attach"; devenv processes down || true; exit 1; }
wait_for_port "$PORT_BETA"  || { echo "FAIL: beta not launched after gamma became ready"; devenv processes down || true; exit 1; }

devenv processes down

echo "All process-up-attach-deps tests passed!"
