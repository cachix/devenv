#!/usr/bin/env bash
set -e

echo "=== Sandbox ReadOnly Test ==="

# This test should FAIL during evaluation because the option is readOnly
# We expect devenv to fail with an error about trying to set a readOnly option

if devenv info &> /dev/null; then
  echo "ERROR: devenv should have failed due to readOnly option being set"
  exit 1
else
  echo "✓ devenv correctly rejected attempt to override readOnly sandbox option"
fi
