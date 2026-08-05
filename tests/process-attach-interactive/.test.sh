#!/usr/bin/env bash

# PTY merge gate for attaching to a detached native process manager.
# Sessions are driven by `devenv-run-tests pty`: stdin is a directive script
# (expect:/send:/run:), so actions follow observed events and every controller
# command is bounded. Inputs are sent once; a dropped command is a test failure.
# Attach streams replay a log backlog, so an expect can consume a stale
# occurrence; markers are unique per session to keep matches live.
# Expected text must lie within one styled span: the TUI emits escape codes
# between UI elements, so a pattern spanning elements never matches.

set -eux

export DEVENV_NO_AI_AGENT=1

PORT_ALPHA=18641
PORT_BETA=18642

runtime_hash() {
  dotfile="$(pwd -P)/.devenv"
  if command -v sha256sum >/dev/null 2>&1; then
    printf '%s' "$dotfile" | sha256sum | cut -c1-7
  else
    printf '%s' "$dotfile" | shasum -a 256 | cut -c1-7
  fi
}

RUNTIME_BASE="${XDG_RUNTIME_DIR:-/tmp}"
PROCESS_RUNTIME_DIR="$RUNTIME_BASE/devenv-$(runtime_hash)/processes"
PID_FILE="$PROCESS_RUNTIME_DIR/native-manager.pid"
SOCKET_FILE="$PROCESS_RUNTIME_DIR/native.sock"

reachable() {
  curl -sf -o /dev/null --connect-timeout 1 "http://127.0.0.1:$1/" 2>/dev/null
}

alpha_process_pid() {
  sh .process-pid.sh "$PORT_ALPHA"
}

dump_failure_state() {
  echo "native manager diagnostics:" >&2
  if [ -f "$PID_FILE" ]; then
    echo "pid file: $(sed -n '1p' "$PID_FILE")" >&2
  else
    echo "pid file: missing" >&2
  fi
  ps -o pid,ppid,stat,command -u "$(id -u)" | grep -E 'devenv|http.server' >&2 || true
  for file in "$PROCESS_RUNTIME_DIR"/*.log "$PROCESS_RUNTIME_DIR"/logs/*; do
    if [ -f "$file" ]; then
      echo "==> $file <==" >&2
      tail -n 80 "$file" >&2 || true
    fi
  done
  for t in ./*.typescript; do
    if [ -f "$t" ]; then
      echo "==> $t <==" >&2
      tail -c 4000 "$t" | tr -d '\000' >&2 || true
      echo >&2
    fi
  done
}

cleanup() {
  status=$?
  trap - EXIT INT TERM
  if [ "$status" -ne 0 ]; then
    dump_failure_state
  fi
  # `devenv processes down` blocks until the daemon and its processes exit.
  devenv processes down >/dev/null 2>&1 || true
  if reachable "$PORT_ALPHA"; then
    echo "alpha port remained bound after cleanup" >&2
    status=1
  fi
  if reachable "$PORT_BETA"; then
    echo "beta port remained bound after cleanup" >&2
    status=1
  fi
  exit "$status"
}
trap cleanup EXIT INT TERM

if reachable "$PORT_ALPHA" || reachable "$PORT_BETA"; then
  echo "test ports are already in use" >&2
  exit 1
fi

# The attach pane shows only the newest log lines, so sessions may only
# expect text that is near the tail: fresh markers, or a "Serving HTTP" the
# session itself provoked.
run_detach_session() {
  output=$1
  command=$2
  marker=$3
  set +e
  devenv-run-tests pty "$output" "$command" >/dev/null <<EOF
expect:Attached to the running process manager
run:curl -s -o /dev/null http://127.0.0.1:$PORT_ALPHA/$marker
expect:$marker
send:\003
expect:Detach
send:\003
EOF
  session_status=$?
  set -e
  case "$session_status" in
    0) ;;
    *)
      echo "detach session exited with status $session_status" >&2
      return "$session_status"
      ;;
  esac
}

devenv up -d >/dev/null 2>&1
devenv processes wait
DAEMON_PID=$(sed -n '1p' "$PID_FILE")
kill -0 "$DAEMON_PID"

# E04: plain interactive `devenv up` attaches and a second Ctrl-C detaches.
# First attach on a fresh daemon: "Serving HTTP" is still the newest alpha
# line, and seeing it guarantees alpha is bound before the marker fetch.
set +e
devenv-run-tests pty plain-up.typescript "devenv up" >/dev/null <<EOF
expect:Attached to the running process manager
expect:Serving HTTP
run:curl -s -o /dev/null http://127.0.0.1:$PORT_ALPHA/plain-up-marker
expect:plain-up-marker
send:\003
expect:Detach
send:\003
EOF
session_status=$?
set -e
case "$session_status" in
  0) ;;
  *) echo "plain up session exited with status $session_status" >&2; exit "$session_status" ;;
esac
grep -a -q "Attached to the running process manager" plain-up.typescript
grep -a -q "plain-up-marker" plain-up.typescript
test "$(sed -n '1p' "$PID_FILE")" = "$DAEMON_PID"
kill -0 "$DAEMON_PID"
reachable "$PORT_ALPHA"

# E05: explicit attach observes the same daemon without scheduling work.
run_detach_session explicit-attach.typescript "devenv processes attach" explicit-attach-marker
grep -a -q "Attached to the running process manager" explicit-attach.typescript
grep -a -q "explicit-attach-marker" explicit-attach.typescript
test "$(sed -n '1p' "$PID_FILE")" = "$DAEMON_PID"
reachable "$PORT_ALPHA"

# R03: repeat real PTY attachments while generating unique logs. Every
# disconnect must release its socket/log tailers, and neither daemon nor child
# PID may change. Fd counts are taken mid-attach so cycles compare like with
# like; each cycle's check observes the previous cycle's residue.
ALPHA_PID=$(alpha_process_pid)
test -n "$ALPHA_PID"
kill -0 "$ALPHA_PID"

set +e
devenv-run-tests pty fd-baseline.typescript "devenv processes attach" >/dev/null <<EOF
expect:Attached to the running process manager
run:sh .fd-count.sh $DAEMON_PID > fd-baseline.txt
send:\003
expect:Detach
send:\003
EOF
session_status=$?
set -e
case "$session_status" in
  0) ;;
  *) echo "fd baseline session exited with status $session_status" >&2; exit "$session_status" ;;
esac
if [ ! -s fd-baseline.txt ]; then
  echo "skipping fd-leak checks: neither /proc nor lsof is available" >&2
fi

for cycle in $(seq 1 5); do
  marker="repeat-attach-marker-$cycle"
  transcript="repeat-attach-$cycle.typescript"
  set +e
  devenv-run-tests pty "$transcript" "devenv processes attach" >/dev/null <<EOF
expect:Attached to the running process manager
run:[ ! -s fd-baseline.txt ] || [ "\$(sh .fd-count.sh $DAEMON_PID)" -le "\$(cat fd-baseline.txt)" ]
run:curl -s -o /dev/null http://127.0.0.1:$PORT_ALPHA/$marker
expect:$marker
send:\003
expect:Detach
send:\003
EOF
  session_status=$?
  set -e
  case "$session_status" in
    0) ;;
    *) echo "attach cycle $cycle exited with status $session_status" >&2; exit "$session_status" ;;
  esac
  grep -a -q "$marker" "$transcript"
  test "$(sed -n '1p' "$PID_FILE")" = "$DAEMON_PID"
  test "$(alpha_process_pid)" = "$ALPHA_PID"
  kill -0 "$DAEMON_PID"
  kill -0 "$ALPHA_PID"
  reachable "$PORT_ALPHA"
done

# E06: attached TUI commands restart, stop, and then re-start the process.
# The PTY verifies selection and sends each key exactly once. Process identity
# is checked out of band because a full-screen redraw can replay old log text.
# The sync marker appearing in alpha's log pane proves the process row is
# rendered before any key is sent; the downs overshoot to the clamped bottom
# row (alpha, always last), and the process-specific footer confirms selection.
set +e
devenv-run-tests pty attached-commands.typescript "devenv processes attach" >/dev/null <<EOF
expect:Attached to the running process manager
run:[ ! -s fd-baseline.txt ] || [ "\$(sh .fd-count.sh $DAEMON_PID)" -le "\$(cat fd-baseline.txt)" ]
run:curl -s -o /dev/null http://127.0.0.1:$PORT_ALPHA/e06-sync-marker
expect:e06-sync-marker
send:\033[B\033[B\033[B\033[B\033[B\033[B
expect:(re)start process
send:\022
run:sh .wait-process.sh changed $PORT_ALPHA $ALPHA_PID
run:sh .process-pid.sh $PORT_ALPHA > e06-restarted-pid.txt
run:curl -sf -o /dev/null http://127.0.0.1:$PORT_ALPHA/
send:\030
run:sh .wait-process.sh absent $PORT_ALPHA
run:! curl -sf -o /dev/null --connect-timeout 1 http://127.0.0.1:$PORT_ALPHA/
send:\022
run:sh .wait-process.sh changed $PORT_ALPHA "\$(cat e06-restarted-pid.txt)"
run:curl -sf -o /dev/null http://127.0.0.1:$PORT_ALPHA/
send:\003
expect:Detach
send:\003
EOF
COMMAND_STATUS=$?
set -e
case "$COMMAND_STATUS" in
  0) ;;
  *) echo "attached command session exited with status $COMMAND_STATUS" >&2; exit "$COMMAND_STATUS" ;;
esac
test "$(sed -n '1p' "$PID_FILE")" = "$DAEMON_PID"
reachable "$PORT_ALPHA"

# E10: automation must fail fast even when a wrapper allocated a PTY.
if devenv up --no-tui </dev/null >non-tty.txt 2>&1; then
  echo "non-TTY up unexpectedly attached" >&2
  exit 1
fi
grep -q "Processes already running" non-tty.txt

if devenv-run-tests pty ci.typescript "CI=1 DEVENV_NO_AI_AGENT=1 devenv up" </dev/null >/dev/null; then
  echo "CI up unexpectedly attached" >&2
  exit 1
fi
grep -a -q "Processes already running" ci.typescript

if devenv-run-tests pty agent.typescript "env -u DEVENV_NO_AI_AGENT CLAUDECODE=1 devenv up" </dev/null >/dev/null; then
  echo "coding-agent up unexpectedly attached" >&2
  exit 1
fi
grep -a -q "Processes already running" agent.typescript
test "$(sed -n '1p' "$PID_FILE")" = "$DAEMON_PID"
reachable "$PORT_ALPHA"

# E11: a newly configured name is unknown to the retained older graph.
cp devenv-with-beta.nix devenv.nix
if devenv processes start beta >config-skew.txt 2>&1; then
  echo "new config process unexpectedly existed in the old manager graph" >&2
  exit 1
fi
grep -q "different configuration" config-skew.txt
grep -q "devenv processes down" config-skew.txt
test "$(sed -n '1p' "$PID_FILE")" = "$DAEMON_PID"
reachable "$PORT_ALPHA"
if reachable "$PORT_BETA"; then
  echo "unknown beta process launched in the old graph" >&2
  exit 1
fi

# E07: choosing `s` from the attached interrupt prompt stops the manager.
# The client's stop blocks until the daemon is gone, so the post-session
# checks are one-shot.
set +e
devenv-run-tests pty stop-manager.typescript "devenv processes attach" >/dev/null <<EOF
expect:Attached to the running process manager
send:\003
expect:Detach
send:s
EOF
STOP_STATUS=$?
set -e
case "$STOP_STATUS" in
  0) ;;
  *) echo "stop-manager session exited with status $STOP_STATUS" >&2; exit "$STOP_STATUS" ;;
esac
if reachable "$PORT_ALPHA"; then
  echo "alpha port still bound after manager stop" >&2
  exit 1
fi
test ! -e "$PID_FILE"
test ! -e "$SOCKET_FILE"

# E08: losing the daemon externally is reported as an error, not a detach.
devenv up -d >/dev/null 2>&1
devenv processes wait
DAEMON_PID=$(sed -n '1p' "$PID_FILE")
set +e
devenv-run-tests pty manager-loss.typescript "devenv processes attach" >/dev/null <<EOF
expect:Attached to the running process manager
run:kill -TERM $DAEMON_PID
expect:process manager went away
EOF
LOSS_STATUS=$?
set -e
test "$LOSS_STATUS" -ne 0
grep -a -q "process manager went away" manager-loss.typescript
if reachable "$PORT_ALPHA"; then
  echo "alpha port still bound after manager loss" >&2
  exit 1
fi
test ! -e "$PID_FILE"
test ! -e "$SOCKET_FILE"

if devenv processes attach >attach-after-down.txt 2>&1; then
  echo "attach unexpectedly succeeded without a manager" >&2
  exit 1
fi
grep -q "No processes running" attach-after-down.txt

echo "All interactive process attach tests passed!"
