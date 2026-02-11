#!/bin/bash
# Test that devenv.local.nix in imported directories is loaded

echo "DEBUG: SHARED_VAR=$SHARED_VAR"
echo "DEBUG: LOCAL_ONLY=$LOCAL_ONLY"
echo "DEBUG: SHARED_BASE=$SHARED_BASE"

if [ "$LOCAL_ONLY" != "yes" ]; then
  echo "FAIL: LOCAL_ONLY should be 'yes' but got '$LOCAL_ONLY'"
  exit 1
fi

if [ "$SHARED_VAR" != "from_local" ]; then
  echo "FAIL: SHARED_VAR should be 'from_local' but got '$SHARED_VAR'"
  exit 1
fi

if [ "$SHARED_BASE" != "true" ]; then
  echo "FAIL: SHARED_BASE should be 'true' but got '$SHARED_BASE'"
  exit 1
fi

echo "PASS: devenv.local.nix from imported directory is loaded correctly"
