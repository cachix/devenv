#!/usr/bin/env bash
# Verify `devenv tasks run` with a process dependency attaches to an already
# running manager (`devenv up -d`) instead of starting a second copy.

set -euo pipefail

. "$DEVENV_TEST_LIB"

export DEVENV_NO_AI_AGENT=1
export DEVENV_RUNTIME="$PWD/.runtime"

state=".devenv/state"
rm -f "$state/dummy.pid" "$state/dummy-ready" "$state/dummy-starts" "$state/repro-ran"

cleanup() {
  status=$?
  trap - EXIT INT TERM
  if [ "$status" -ne 0 ]; then
    ps -o pid,ppid,stat,command -u "$(id -u)" | grep -E 'devenv|sleep' >&2 || true
    devenv processes list >&2 || true
    for file in .runtime/processes/daemon.log .runtime/processes/logs/*; do
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

devenv up -d
devenv processes wait

if [ ! -f "$state/dummy.pid" ]; then
  echo "FAIL: dummy never recorded its pid"
  exit 1
fi

pid_before=$(cat "$state/dummy.pid")
kill -0 "$pid_before" || {
  echo "FAIL: dummy pid $pid_before is not running after up -d"
  exit 1
}

starts_before=$(cat "$state/dummy-starts")
if [ "$starts_before" != "1" ]; then
  echo "FAIL: dummy should have started once via devenv up, got $starts_before"
  exit 1
fi

run_bounded 120 devenv tasks run test:repro

if [ ! -f "$state/repro-ran" ]; then
  echo "FAIL: test:repro did not run"
  exit 1
fi

pid_after=$(cat "$state/dummy.pid")
if [ "$pid_before" != "$pid_after" ]; then
  echo "FAIL: dummy was restarted (pid $pid_before -> $pid_after)"
  exit 1
fi

kill -0 "$pid_before" || {
  echo "FAIL: dummy was stopped by tasks run"
  exit 1
}

starts_after=$(cat "$state/dummy-starts")
if [ "$starts_after" != "1" ]; then
  echo "FAIL: dummy was started again by tasks run (starts=$starts_after)"
  exit 1
fi

devenv processes list | grep -E '^dummy\b' | grep -q ready || {
  echo "FAIL: dummy is not ready after tasks run:"
  devenv processes list
  exit 1
}

devenv processes list | grep -E '^dummy\b' | grep -q 'restarts: 0' || {
  echo "FAIL: dummy was restart-looped by tasks run:"
  devenv processes list
  exit 1
}

devenv processes down
trap - EXIT INT TERM

echo "PASS: tasks run reused the already-running process without restarting it"
