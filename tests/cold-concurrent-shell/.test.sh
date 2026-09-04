#!/usr/bin/env bash
# Concurrent cold `devenv shell` entries into a project that has no `.devenv/`
# yet must all succeed (#3133).
set -euo pipefail

run_concurrent() {
  local n="$1"
  local label="$2"
  local failed=0
  local pids=()
  local logs=()
  local i status

  echo "${label}: entering ${n} shells at once"

  for _ in $(seq 1 "$n"); do
    local log
    log="$(mktemp)"
    logs+=("$log")
    devenv shell -q -- true >"$log" 2>&1 &
    pids+=("$!")
  done

  for i in $(seq 1 "$n"); do
    if wait "${pids[$((i - 1))]}"; then
      status=0
    else
      status=$?
      failed=$((failed + 1))
    fi
    printf '  shell %-3d exit=%s\n' "$i" "$status"
  done

  for i in $(seq 1 "$n"); do
    if [ -s "${logs[$((i - 1))]}" ]; then
      echo "── ${label} shell $i ──"
      cat "${logs[$((i - 1))]}"
    fi
  done
  rm -f "${logs[@]}"

  if [ "$failed" -gt 0 ]; then
    echo "FAIL: ${failed} of ${n} ${label} entries died; every entry should succeed" >&2
    return 1
  fi
}

rm -rf .devenv
run_concurrent 2 "cold .devenv"

# Warm path must keep working once `.devenv/` exists.
run_concurrent 2 "warm .devenv"

echo "ok: concurrent cold and warm entries succeeded"
