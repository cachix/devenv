#!/usr/bin/env bash
set -euo pipefail

# Clean up
rm -f /tmp/devenv-test-shell-entered.txt

# Run shell with a failing task before enterShell
# The shell command should succeed (non-fatal task failures)
# even though the enterShell task itself is marked as dependency-failed
if devenv shell -- echo "SHELL_COMMAND_RAN" 2>&1; then
  echo "Shell correctly entered despite task failure"
else
  echo "ERROR: Shell command failed (task failures should be non-fatal)"
  exit 1
fi

# Note: The enterShell script doesn't run because its dependency failed
# (hard dependency via `before`). But the shell still enters.
# This test verifies that task failures don't block shell entry.

# Clean up
rm -f /tmp/devenv-test-shell-entered.txt
