#!/bin/sh
# Wait up to 25s for the HTTP-server process on $2 to be absent, present, or
# different from the old PID in $3. This is the state barrier used after a TUI
# command; terminal output is only a rendered projection and may replay old
# log lines on every redraw.
mode=$1
port=$2
old_pid=${3:-}

for _ in $(seq 1 125); do
  pid=$(sh .process-pid.sh "$port")
  case "$mode" in
    absent)
      if [ -z "$pid" ]; then
        exit 0
      fi
      ;;
    present)
      if [ -n "$pid" ]; then
        exit 0
      fi
      ;;
    changed)
      if [ -n "$pid" ] && [ "$pid" != "$old_pid" ]; then
        exit 0
      fi
      ;;
    *)
      echo "unknown process wait mode: $mode" >&2
      exit 2
      ;;
  esac
  sleep 0.2
done

echo "timed out waiting for process on port $port to become $mode (old PID: ${old_pid:-none})" >&2
exit 1
