#!/usr/bin/env bash
# Verify `devenv tasks run` against a manager started with `devenv up -d`:
#
#  - a healthy process dependency is reused, not restarted or stopped (#3137);
#  - a process selected only through `wantedBy` is started by the manager, and
#    one that opts out with `wantedBy = [ ]` is not;
#  - `@completed` waits while the owned worker runs and is satisfied by the
#    manager's preserved exit afterwards, without running the worker again.

set -euo pipefail

. "$DEVENV_TEST_LIB"

export DEVENV_NO_AI_AGENT=1
export DEVENV_RUNTIME="$PWD/.runtime"

state=".devenv/state"
rm -f "$state"/*-starts "$state"/*-ran "$state/dummy.pid" "$state/worker-release"

cleanup() {
  status=$?
  trap - EXIT INT TERM
  if [ "$status" -ne 0 ]; then
    ps -o pid,ppid,stat,command -u "$(id -u)" | grep -E 'devenv|sleep' >&2 || true
    devenv processes list >&2 || true
    for file in completed.log .runtime/processes/daemon.log .runtime/processes/logs/*; do
      if [ -f "$file" ]; then
        echo "==> $file <==" >&2
        tail -n 80 "$file" >&2 || true
      fi
    done
  fi
  devenv processes down >/dev/null 2>&1 || true
  exit "$status"
}
trap cleanup EXIT INT TERM

fail() {
  echo "FAIL: $*" >&2
  devenv processes list >&2 || true
  exit 1
}

starts() {
  cat "$state/$1-starts" 2>/dev/null || echo 0
}

phase_is() {
  devenv processes list | grep -E "^$1\b" | grep -q "$2"
}

# Leave `wanted` and `optout` to the task runs below.
devenv up -d dummy worker
devenv processes wait

[ -f "$state/dummy.pid" ] || fail "dummy never recorded its pid"
pid_before=$(cat "$state/dummy.pid")
kill -0 "$pid_before" || fail "dummy pid $pid_before is not running after up -d"
[ "$(starts dummy)" = 1 ] || fail "dummy should have started once via devenv up, got $(starts dummy)"
[ "$(starts worker)" = 1 ] || fail "worker should have started once via devenv up, got $(starts worker)"

echo "--- a healthy process dependency is reused"
run_bounded 120 devenv tasks run test:repro
[ -f "$state/repro-ran" ] || fail "test:repro did not run"
[ "$(cat "$state/dummy.pid")" = "$pid_before" ] || fail "dummy was restarted"
kill -0 "$pid_before" || fail "dummy was stopped by tasks run"
[ "$(starts dummy)" = 1 ] || fail "dummy was started again by tasks run (starts=$(starts dummy))"
phase_is dummy ready || fail "dummy is not ready after tasks run"
phase_is dummy 'restarts: 0' || fail "dummy was restart-looped by tasks run"

echo "--- wantedBy selects a process; wantedBy = [ ] opts one out"
run_bounded 120 devenv tasks run test:wantedby
[ -f "$state/wantedby-ran" ] || fail "test:wantedby did not run"
[ "$(starts wanted)" = 1 ] || fail "wanted should have been started once, got $(starts wanted)"
phase_is wanted ready || fail "wanted is not running in the manager after tasks run"
[ "$(starts optout)" = 0 ] || fail "optout opted out of selection but was started"
phase_is optout ready && fail "optout is running"

echo "--- @completed waits while the worker runs"
run_bounded 120 devenv tasks run test:completed > completed.log 2>&1 &
completed_run=$!
sleep 3
[ -f "$state/completed-ran" ] && fail "@completed ran while the worker was still running"
kill -0 "$completed_run" 2>/dev/null || fail "tasks run exited while its @completed dependency was running"

touch "$state/worker-release"
wait "$completed_run" || fail "tasks run failed after the worker exited"
[ -f "$state/completed-ran" ] || fail "test:completed did not run after the worker exited"
phase_is worker exited || fail "worker did not exit in the manager"

echo "--- @completed is satisfied by the worker's preserved exit"
rm -f "$state/completed-ran"
run_bounded 120 devenv tasks run test:completed
[ -f "$state/completed-ran" ] || fail "test:completed did not run against the exited worker"
[ "$(starts worker)" = 1 ] || fail "the exited worker was run again (starts=$(starts worker))"

echo "--- every owned process is still running"
kill -0 "$pid_before" || fail "dummy was stopped by a task run"
[ "$(starts dummy)" = 1 ] || fail "dummy was restarted by a task run"
phase_is wanted ready || fail "wanted was stopped by a task run"

devenv processes down
trap - EXIT INT TERM

echo "PASS: tasks run used the running manager's processes and dependency state"
