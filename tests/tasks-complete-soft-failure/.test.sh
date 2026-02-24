#!/usr/bin/env bash
set -euo pipefail

# Test 1: devenv tasks run should exit 0 when only @completed deps fail
if devenv tasks run devenv:enterShell --mode all 2>&1; then
  echo "PASS: tasks run exited 0 with only @completed failure"
else
  echo "FAIL: tasks run exited non-zero despite only @completed failure"
  exit 1
fi

# Test 2: devenv shell should enter successfully
if devenv shell -- echo "SHELL_OK" 2>&1; then
  echo "PASS: shell entered despite @completed task failure"
else
  echo "FAIL: shell failed to enter"
  exit 1
fi
