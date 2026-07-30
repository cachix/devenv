#!/usr/bin/env bash

# PTY merge gate for attaching to a detached native process manager.
# PTY sessions are driven by .pty-run.py (util-linux `script -qefc` is not
# available on macOS).

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

wait_for_port() {
  port=$1
  for _ in $(seq 1 60); do
    if reachable "$port"; then
      return 0
    fi
    sleep 1
  done
  return 1
}

wait_for_port_free() {
  port=$1
  for _ in $(seq 1 30); do
    if ! reachable "$port"; then
      return 0
    fi
    sleep 1
  done
  return 1
}

wait_for_manager() {
  for _ in $(seq 1 60); do
    if [ -s "$PID_FILE" ] && devenv processes wait >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  return 1
}

# Count the manager's open fds: /proc on Linux, lsof on macOS. Empty output
# means no source produced a count and the fd-leak checks are skipped.
manager_fd_count() {
  if [ -d "/proc/$DAEMON_PID/fd" ]; then
    find "/proc/$DAEMON_PID/fd" -mindepth 1 -maxdepth 1 -type l 2>/dev/null | wc -l | tr -d ' '
  elif lsof=$(command -v lsof) || { lsof=/usr/sbin/lsof && [ -x "$lsof" ]; }; then
    # Capture before counting: a failed lsof piped straight into wc would
    # read as a healthy count of 0 and turn the leak check into a no-op.
    if fd_listing=$("$lsof" -np "$DAEMON_PID" 2>/dev/null); then
      printf '%s\n' "$fd_listing" | awk 'NR > 1 && $4 ~ /^[0-9]/' | wc -l | tr -d ' '
    fi
  fi
}

wait_for_manager_fd_count_at_most() {
  maximum=$1
  for _ in $(seq 1 50); do
    current=$(manager_fd_count)
    if [ -n "$current" ] && [ "$current" -le "$maximum" ]; then
      return 0
    fi
    sleep 0.1
  done
  return 1
}

alpha_process_pid() {
  needle="python3 -u -m http.server $PORT_ALPHA"
  ps -eo pid=,args= |
    awk -v needle="$needle" '
      index($0, needle) && !index($0, "awk -v needle=") {
        print $1
        exit
      }
    '
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
}

cleanup() {
  status=$?
  trap - EXIT INT TERM
  if [ "$status" -ne 0 ]; then
    dump_failure_state
  fi
  devenv processes down >/dev/null 2>&1 || true
  if ! wait_for_port_free "$PORT_ALPHA"; then
    echo "alpha port remained bound after cleanup" >&2
    status=1
  fi
  if ! wait_for_port_free "$PORT_BETA"; then
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

run_detach_session() {
  output=$1
  command=$2
  marker=${3:-attach-live-marker}
  set +e
  (
    sleep 3
    curl -sf -o /dev/null "http://127.0.0.1:$PORT_ALPHA/$marker" || true
    sleep 2
    printf '\003'
    sleep 1
    printf '\003'
  ) | timeout 45 python3 .pty-run.py "$output" "$command" >/dev/null
  session_status=$?
  set -e
  case "$session_status" in
    0|130) ;;
    *)
      echo "detach session exited with status $session_status" >&2
      return "$session_status"
      ;;
  esac
}

devenv up -d >/dev/null 2>&1
wait_for_manager
wait_for_port "$PORT_ALPHA"
DAEMON_PID=$(sed -n '1p' "$PID_FILE")
kill -0 "$DAEMON_PID"

# E04: plain interactive `devenv up` attaches and a second Ctrl-C detaches.
run_detach_session plain-up.typescript "devenv up"
grep -a -q "Attached to the running process manager" plain-up.typescript
grep -a -q "attach-live-marker" plain-up.typescript
test "$(sed -n '1p' "$PID_FILE")" = "$DAEMON_PID"
kill -0 "$DAEMON_PID"
reachable "$PORT_ALPHA"

# E05: explicit attach observes the same daemon without scheduling work.
run_detach_session explicit-attach.typescript "devenv processes attach"
grep -a -q "Attached to the running process manager" explicit-attach.typescript
grep -a -q "attach-live-marker" explicit-attach.typescript
test "$(sed -n '1p' "$PID_FILE")" = "$DAEMON_PID"
reachable "$PORT_ALPHA"

# R03: repeat real PTY attachments while generating unique logs. Every
# disconnect must release its socket/log tailers, and neither daemon nor child
# PID may change. Protocol-level tests assert each unique line arrives once;
# this checks the OS resource boundary through /proc.
ALPHA_PID=$(alpha_process_pid)
test -n "$ALPHA_PID"
kill -0 "$ALPHA_PID"
sleep 1
FD_BASELINE=$(manager_fd_count)
if [ -z "$FD_BASELINE" ]; then
  echo "skipping fd-leak checks: neither /proc nor lsof is available" >&2
fi
for cycle in $(seq 1 5); do
  marker="repeat-attach-marker-$cycle"
  transcript="repeat-attach-$cycle.typescript"
  run_detach_session "$transcript" "devenv processes attach" "$marker"
  grep -a -q "$marker" "$transcript"
  test "$(sed -n '1p' "$PID_FILE")" = "$DAEMON_PID"
  test "$(alpha_process_pid)" = "$ALPHA_PID"
  kill -0 "$DAEMON_PID"
  kill -0 "$ALPHA_PID"
  reachable "$PORT_ALPHA"
  if [ -n "$FD_BASELINE" ] && ! wait_for_manager_fd_count_at_most "$FD_BASELINE"; then
    echo "manager file descriptors grew from $FD_BASELINE to $(manager_fd_count) after attach cycle $cycle" >&2
    exit 1
  fi
done

# E06: attached TUI commands restart, stop, and then re-start the process.
set +e
(
  sleep 3
  printf '\033[B\033[B'
  sleep 1
  printf '\022'
  sleep 3
  printf '\030'
  sleep 4
  if ! reachable "$PORT_ALPHA"; then
    touch attached-stop-ok
  fi
  printf '\022'
  sleep 4
  if reachable "$PORT_ALPHA"; then
    touch attached-restart-ok
  fi
  printf '\003'
  sleep 1
  printf '\003'
) | timeout 60 python3 .pty-run.py attached-commands.typescript "devenv processes attach" >/dev/null
COMMAND_STATUS=$?
set -e
case "$COMMAND_STATUS" in
  0|130) ;;
  *) echo "attached command session exited with status $COMMAND_STATUS" >&2; exit "$COMMAND_STATUS" ;;
esac
test -f attached-stop-ok
test -f attached-restart-ok
test "$(sed -n '1p' "$PID_FILE")" = "$DAEMON_PID"

# E10: automation must fail fast even when a wrapper allocated a PTY.
if timeout 15 devenv up --no-tui </dev/null >non-tty.txt 2>&1; then
  echo "non-TTY up unexpectedly attached" >&2
  exit 1
fi
grep -q "Processes already running" non-tty.txt

if timeout 15 python3 .pty-run.py ci.typescript "CI=1 DEVENV_NO_AI_AGENT=1 devenv up" </dev/null >/dev/null; then
  echo "CI up unexpectedly attached" >&2
  exit 1
fi
grep -a -q "Processes already running" ci.typescript

if timeout 15 python3 .pty-run.py agent.typescript "env -u DEVENV_NO_AI_AGENT CLAUDECODE=1 devenv up" </dev/null >/dev/null; then
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
set +e
(
  sleep 3
  printf '\003'
  sleep 1
  printf 's'
) | timeout 45 python3 .pty-run.py stop-manager.typescript "devenv processes attach" >/dev/null
STOP_STATUS=$?
set -e
case "$STOP_STATUS" in
  0|130) ;;
  *) echo "stop-manager session exited with status $STOP_STATUS" >&2; exit "$STOP_STATUS" ;;
esac
wait_for_port_free "$PORT_ALPHA"
test ! -e "$PID_FILE"
test ! -e "$SOCKET_FILE"

# E08: losing the daemon externally is reported as an error, not a detach.
devenv up -d >/dev/null 2>&1
wait_for_manager
wait_for_port "$PORT_ALPHA"
DAEMON_PID=$(sed -n '1p' "$PID_FILE")
if (
  sleep 3
  kill -TERM "$DAEMON_PID"
) | timeout 45 python3 .pty-run.py manager-loss.typescript "devenv processes attach" >/dev/null; then
  echo "attach unexpectedly succeeded after manager loss" >&2
  exit 1
fi
grep -a -q "process manager went away" manager-loss.typescript
wait_for_port_free "$PORT_ALPHA"
test ! -e "$PID_FILE"
test ! -e "$SOCKET_FILE"

if devenv processes attach >attach-after-down.txt 2>&1; then
  echo "attach unexpectedly succeeded without a manager" >&2
  exit 1
fi
grep -q "No processes running" attach-after-down.txt

echo "All interactive process attach tests passed!"
