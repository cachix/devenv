#!/usr/bin/env bash
set -xe
set -o pipefail

proj_root="$(pwd)"
mkdir -p sub/nested

# Discovery from one level deep
(cd sub && devenv print-paths | grep -Fxq "DEVENV_ROOT=\"$proj_root\"")

# Discovery from two levels deep
(cd sub/nested && devenv print-paths | grep -Fxq "DEVENV_ROOT=\"$proj_root\"")

# Negative case: outside any project, original error preserved
outside="$(mktemp -d)"
trap 'rm -rf "$outside"' EXIT
output="$(cd "$outside" && devenv print-paths 2>&1 || true)"
if echo "$output" | grep -q "devenv.nix does not exist"; then
  echo "✓ negative case: error preserved outside any project"
else
  echo "expected 'devenv.nix does not exist' outside any project, got:"
  echo "$output"
  exit 1
fi
