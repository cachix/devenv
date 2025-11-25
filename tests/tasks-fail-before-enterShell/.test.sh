#!/usr/bin/env bash
set -euo pipefail

# Clean up
rm -f /tmp/devenv-test-shell-entered.txt

# Try to enter the shell and run "true" (this should fail because of the failing task)
# Using "-- true" ensures the shell runs the shellHook (which includes tasks) before executing the command
if devenv shell -- true 2>&1; then
  # Shell entered successfully - this is the bug!
  if [ -f /tmp/devenv-test-shell-entered.txt ]; then
    echo "BUG: Shell entered even though dependency task failed"
    cat /tmp/devenv-test-shell-entered.txt
    exit 1
  else
    echo "Shell command succeeded but our enterShell didn't run"
    exit 1
  fi
else
  # Shell failed to enter - this is expected
  if [ -f /tmp/devenv-test-shell-entered.txt ]; then
    echo "Shell command failed but enterShell still ran"
    cat /tmp/devenv-test-shell-entered.txt
    exit 1
  else
    echo "Shell correctly failed to enter when dependency task failed"
  fi
fi

# Clean up
rm -f /tmp/devenv-test-shell-entered.txt
