#!/usr/bin/env bash
set -euo pipefail

# Set env vars for clean mode to filter.
# CLEAN_TEST_VAR is in clean.keep — should survive.
# DATABASE_URL is NOT in clean.keep — should be stripped.
export CLEAN_TEST_VAR=keep-me
export DATABASE_URL=postgres://should-be-stripped

# Check that CLEAN_TEST_VAR is preserved
devenv shell -- bash -c 'test -n "$CLEAN_TEST_VAR"'
echo "PASS: CLEAN_TEST_VAR is preserved"

# Check that DATABASE_URL is stripped
if devenv shell -- bash -c 'test -n "${DATABASE_URL:-}"' 2>/dev/null; then
    echo "FAIL: DATABASE_URL should have been stripped"
    exit 1
fi
echo "PASS: DATABASE_URL is stripped"
