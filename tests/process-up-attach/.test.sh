#!/usr/bin/env bash

# Verify `devenv up` attaches to a running native manager instead of failing,
# and reports truthfully what it did.
#
#  - `devenv up -d` starts alpha + beta.
#  - a second `devenv up -d` while everything runs reports both as already
#    running (exit 0, nothing restarted).
#  - stopping beta leaves alpha running; another `devenv up -d` attaches and
#    reports beta as scheduled.
#  - an attaching `devenv up -d beta` honours the subset and only restarts
#    beta.
#  - `devenv up -d` refuses to schedule into a foreground `devenv up` session
#    owned by another terminal.
#  - `devenv processes attach` fails fast when no manager is running.

set -ex

# The assertions below grep info-level messages ("Scheduled: ...", "Already
# running ..."); the AI-agent auto-quiet mode would suppress them, so opt out.
export DEVENV_NO_AI_AGENT=1
export DEVENV_RUNTIME="$PWD/.runtime"

PORT_A=
PORT_B=
PORT_A_FILE=.devenv/state/alpha-port
PORT_B_FILE=.devenv/state/beta-port

load_ports() {
  for _ in $(seq 1 30); do
    if [ -s "$PORT_A_FILE" ] && [ -s "$PORT_B_FILE" ]; then
      read -r PORT_A < "$PORT_A_FILE"
      read -r PORT_B < "$PORT_B_FILE"
      return 0
    fi
    sleep 1
  done
  return 1
}

load_alpha_port() {
  for _ in $(seq 1 30); do
    if [ -s "$PORT_A_FILE" ]; then
      read -r PORT_A < "$PORT_A_FILE"
      return 0
    fi
    sleep 1
  done
  return 1
}

load_beta_port() {
  for _ in $(seq 1 30); do
    if [ -s "$PORT_B_FILE" ]; then
      read -r PORT_B < "$PORT_B_FILE"
      return 0
    fi
    sleep 1
  done
  return 1
}

reset_port_files() {
  PORT_A=
  PORT_B=
  rm -f "$PORT_A_FILE" "$PORT_B_FILE"
}

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

# Poll until the running manager's pid file exists and it reports ready.
# `devenv processes wait` fails fast while the pid file is absent (the
# foreground session writes it only once its cold start completes), so retry
# it; once the pid file exists it blocks until ready. Event-driven with a
# bounded number of attempts.
wait_for_manager() {
  for _ in $(seq 1 60); do
    if devenv processes wait 2>/dev/null; then return 0; fi
    sleep 1
  done
  return 1
}

cleanup() {
  status=$?
  trap - EXIT INT TERM
  if [ "$status" -ne 0 ]; then
    ps -o pid,ppid,stat,command -u "$(id -u)" | grep -E 'devenv|http.server' >&2 || true
    for file in .runtime/processes/daemon.log .runtime/processes/logs/*; do
      if [ -f "$file" ]; then
        echo "==> $file <==" >&2
        tail -n 80 "$file" >&2 || true
      fi
    done
  fi
  devenv processes down >/dev/null 2>&1 || true
  if [ -n "$PORT_A" ]; then
    wait_for_port_free "$PORT_A" || status=1
  fi
  if [ -n "$PORT_B" ]; then
    wait_for_port_free "$PORT_B" || status=1
  fi
  exit "$status"
}
trap cleanup EXIT INT TERM

# Start both processes.
reset_port_files
devenv up -d
load_ports
devenv processes wait
wait_for_port "$PORT_A" || { echo "FAIL: alpha did not start"; devenv processes down || true; exit 1; }
wait_for_port "$PORT_B" || { echo "FAIL: beta did not start"; devenv processes down || true; exit 1; }

# Second up while everything is already running: exit 0, truthful "already
# running" report, nothing restarted.
devenv up -d >reup_all.txt 2>&1
grep -q "Already running" reup_all.txt || {
  echo "FAIL: re-up with everything running should report already running; got:"
  cat reup_all.txt
  devenv processes down || true; exit 1;
}

# Stop beta; alpha stays up.
devenv processes stop beta
wait_for_port_free "$PORT_B" || { echo "FAIL: beta did not stop"; devenv processes down || true; exit 1; }
reachable "$PORT_A" || { echo "FAIL: alpha died after stopping beta"; devenv processes down || true; exit 1; }

# Third up attaches and restarts the up-enabled beta, reporting it scheduled.
devenv up -d >reup.txt 2>&1
grep -q "Scheduled: beta" reup.txt || {
  echo "FAIL: attaching up should report beta as scheduled; got:"
  cat reup.txt
  devenv processes down || true; exit 1;
}
devenv processes wait
wait_for_port "$PORT_B" || { echo "FAIL: beta not restarted by attaching up"; devenv processes down || true; exit 1; }
reachable "$PORT_A" || { echo "FAIL: alpha died after attaching up"; devenv processes down || true; exit 1; }

# Stop beta; an attaching `devenv up -d beta` must honour the subset and only
# (re)start beta, not alpha (which is already running) and not anything else.
devenv processes stop beta
wait_for_port_free "$PORT_B" || { echo "FAIL: beta did not stop"; devenv processes down || true; exit 1; }
devenv up -d beta >subset.txt 2>&1
grep -q "Scheduled: beta" subset.txt || {
  echo "FAIL: subset attach should report beta as scheduled; got:"
  cat subset.txt
  devenv processes down || true; exit 1;
}
devenv processes wait
wait_for_port "$PORT_B" || { echo "FAIL: beta not restarted by subset attach"; devenv processes down || true; exit 1; }
reachable "$PORT_A" || { echo "FAIL: alpha died after subset attach"; devenv processes down || true; exit 1; }

devenv processes down
wait_for_port_free "$PORT_A" || { echo "FAIL: alpha still bound after down"; exit 1; }
wait_for_port_free "$PORT_B" || { echo "FAIL: beta still bound after down"; exit 1; }

# Foreground ownership guard: a `devenv up -d` must refuse to schedule into a
# foreground `devenv up` session owned by another terminal — that session owns
# its processes, and its exit tears them down.
reset_port_files
devenv up --no-tui >fg.log 2>&1 &
FG_PID=$!
wait_for_manager || {
  echo "FAIL: foreground up did not become ready; log:"
  cat fg.log
  kill -INT "$FG_PID" 2>/dev/null || true
  exit 1
}
load_ports
if devenv up -d >fgup.txt 2>&1; then
  echo "FAIL: up -d should refuse against a foreground up session; got:"
  cat fgup.txt
  kill -INT "$FG_PID" 2>/dev/null || true
  exit 1
fi
grep -q "foreground" fgup.txt || {
  echo "FAIL: unexpected error refusing the foreground session; got:"
  cat fgup.txt
  kill -INT "$FG_PID" 2>/dev/null || true
  exit 1
}
# Detach cleanup: SIGINT stops the foreground session and its processes.
kill -INT "$FG_PID"
wait "$FG_PID" || true
wait_for_port_free "$PORT_A" || { echo "FAIL: alpha still bound after foreground up exit"; exit 1; }
wait_for_port_free "$PORT_B" || { echo "FAIL: beta still bound after foreground up exit"; exit 1; }

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

# `devenv processes start <name>` with no manager running cold-starts one in
# the background launching only the named process; the manager still owns the
# full process set (beta is registered, visible as not started), so a
# follow-up named start schedules into it over the socket instead of erroring
# or spawning a second manager.
reset_port_files
devenv processes start alpha
wait_for_manager || { echo "FAIL: processes start did not cold-start a manager"; exit 1; }
load_alpha_port
wait_for_port "$PORT_A" || { echo "FAIL: alpha not started by cold processes start"; devenv processes down || true; exit 1; }
if [ -e "$PORT_B_FILE" ]; then
  echo "FAIL: beta started by cold 'processes start alpha'"
  devenv processes down || true
  exit 1
fi
devenv processes list | grep -q beta || { echo "FAIL: beta missing from list after cold subset start"; devenv processes down || true; exit 1; }
devenv processes start beta
devenv processes wait
load_beta_port
wait_for_port "$PORT_B" || { echo "FAIL: beta not started over the socket"; devenv processes down || true; exit 1; }
devenv processes down
wait_for_port_free "$PORT_A" || { echo "FAIL: alpha still bound after final down"; exit 1; }
wait_for_port_free "$PORT_B" || { echo "FAIL: beta still bound after final down"; exit 1; }

echo "All process-up-attach tests passed!"
