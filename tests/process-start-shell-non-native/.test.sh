#!/usr/bin/env bash

# `start.enable = "shell"` with a non-native process manager must fail fast with
# a clear assertion error rather than silently misbehaving.

set -u

if devenv up -d --no-tui >out.txt 2>&1; then
  echo "FAIL: expected an assertion error, but 'devenv up' succeeded"
  cat out.txt
  devenv down --no-tui >/dev/null 2>&1 || true
  exit 1
fi

if ! grep -q "requires the native process manager" out.txt; then
  echo "FAIL: expected the native-manager assertion message"
  cat out.txt
  exit 1
fi

echo "PASS: start.enable = \"shell\" rejected with non-native process manager"
