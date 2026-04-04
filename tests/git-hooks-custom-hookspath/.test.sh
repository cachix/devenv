#!/usr/bin/env bash
set -euo pipefail

output=$(devenv shell -- true 2>&1 || true)

if [ "$(git config --get core.hooksPath 2>/dev/null || true)" != "$PWD/custom-hooks" ]; then
  echo "core.hooksPath was unexpectedly changed"
  exit 1
fi

if [ -e "$PWD/custom-hooks/pre-commit" ]; then
  echo "pre-commit hook should not be installed into custom hooksPath"
  exit 1
fi

echo "$output" | grep -q 'Cowardly refusing to install hooks with `core.hooksPath` set.' || {
  echo "expected prek refusal message not found"
  echo "$output"
  exit 1
}
