#!/usr/bin/env bash
set -euo pipefail

if output=$(devenv shell -- true 2>&1); then
  echo "ERROR: devenv shell succeeded but should fail for invalid themes path"
  exit 1
fi

echo "$output" | grep -q '`opencode.themes` must be a directory when set to a path.' || {
  echo "ERROR: Expected themes assertion message not found"
  echo "Output was:"
  echo "$output"
  exit 1
}

echo "✓ Invalid themes path assertion works"
