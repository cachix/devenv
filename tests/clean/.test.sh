#!/usr/bin/env bash
set -euo pipefail

# CLEAN_TEST_VAR is in clean.keep — should survive.
# DATABASE_URL is NOT in clean.keep — should be stripped.
export CLEAN_TEST_VAR=keep-me
export DATABASE_URL=postgres://should-be-stripped

devenv shell -- bash -c '
  if [ -z "${CLEAN_TEST_VAR:-}" ]; then
    echo "FAIL: CLEAN_TEST_VAR is not set"
    exit 1
  fi
  if [ -n "${DATABASE_URL:-}" ]; then
    echo "FAIL: DATABASE_URL should have been stripped"
    exit 1
  fi
'
