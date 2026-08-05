#!/bin/sh
# Print the open-fd count of $1: /proc on Linux, lsof on macOS. No output
# means neither source worked and fd checks should be skipped.
pid=$1
if [ -d "/proc/$pid/fd" ]; then
  find "/proc/$pid/fd" -mindepth 1 -maxdepth 1 -type l 2>/dev/null | wc -l | tr -d ' '
elif lsof=$(command -v lsof) || { lsof=/usr/sbin/lsof && [ -x "$lsof" ]; }; then
  # Capture before counting: a failed lsof piped straight into wc would
  # read as a healthy count of 0 and turn the leak check into a no-op.
  if fd_listing=$("$lsof" -np "$pid" 2>/dev/null); then
    printf '%s\n' "$fd_listing" | awk 'NR > 1 && $4 ~ /^[0-9]/' | wc -l | tr -d ' '
  fi
fi
