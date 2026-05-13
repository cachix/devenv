#!/usr/bin/env bash
# Regression test for https://github.com/cachix/devenv/issues/2820
# A syntax error in devenv.nix should produce a useful error message
# (mentioning "syntax error") rather than an unrelated stale warning.

set -uo pipefail

output=$(devenv shell -- true 2>&1)
status=$?

if [ "$status" -eq 0 ]; then
    echo "Test failed: devenv shell should have failed but exited 0"
    echo "Output: $output"
    exit 1
fi

if ! echo "$output" | grep -qi "syntax error"; then
    echo "Test failed: error output does not mention 'syntax error'"
    echo "Output: $output"
    exit 1
fi

if ! echo "$output" | grep -q "devenv\.nix"; then
    echo "Test failed: error output does not reference devenv.nix"
    echo "Output: $output"
    exit 1
fi

echo "OK: syntax error correctly reported"
