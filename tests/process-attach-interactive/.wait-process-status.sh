#!/bin/sh
# Wait for the native process manager's public status API to report an exact
# lifecycle phase. Command completion is asserted separately in the PTY, while
# this helper verifies the resulting authoritative manager state.
name=$1
expected=$2

for _ in $(seq 1 125); do
  status=$(devenv processes status "$name" 2>&1) || status=
  phase=$(printf '%s\n' "$status" | sed -n 's/^Phase:[[:space:]]*//p')
  if [ "$phase" = "$expected" ]; then
    exit 0
  fi
  sleep 0.2
done

echo "timed out waiting for process $name to become $expected" >&2
devenv processes status "$name" >&2 || true
exit 1
