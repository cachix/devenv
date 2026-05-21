#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

SYSTEM=${SYSTEM:-$(nix eval --impure --raw --expr 'builtins.currentSystem')}

fixtures=$(nix eval --json ".#nixosTests.${SYSTEM}" --apply 'builtins.attrNames' | tr -d '[]"' | tr ',' ' ')

if [ -z "${fixtures// }" ]; then
  echo "no fixtures discovered for ${SYSTEM}"
  exit 1
fi

failed=()
for f in $fixtures; do
  echo
  echo "==> $f"
  if ! nix build --no-link ".#nixosTests.${SYSTEM}.${f}"; then
    failed+=("$f")
  fi
done

echo
if [ ${#failed[@]} -gt 0 ]; then
  echo "FAILED: ${failed[*]}"
  exit 1
fi
echo "all fixtures passed"
