#!/usr/bin/env bash
set -euo pipefail

if output=$(devenv shell -- true 2>&1); then
  echo "ERROR: devenv shell succeeded but should fail for invalid tools path"
  exit 1
fi

echo "$output" | grep -q '`opencode.tools` must be a directory when set to a path.' || {
  echo "ERROR: Expected tools assertion message not found"
  echo "Output was:"
  echo "$output"
  exit 1
}

echo "✓ Invalid tools path assertion works"
