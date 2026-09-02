#!/usr/bin/env bash
set -euo pipefail

cleanup() {
  devenv processes down >/dev/null 2>&1 || true
  rm -f process-ran.txt task-ran.txt
}
trap cleanup EXIT

rm -f process-ran.txt task-ran.txt

devenv tasks run --no-tui repro:build

if [ ! -f process-ran.txt ]; then
  echo "FAIL: process-ran.txt missing — start.enable=false process was not run as a @completed dependency"
  exit 1
fi
if [ ! -f task-ran.txt ]; then
  echo "FAIL: task-ran.txt missing — dependent task did not run"
  exit 1
fi
echo "PASS: @completed ran start.enable=false process"

rm -f process-ran.txt task-ran.txt
devenv up -d
devenv processes wait --timeout 30

if [ -f process-ran.txt ]; then
  echo "FAIL: process-ran.txt exists after devenv up — start.enable=false process was auto-started"
  exit 1
fi
echo "PASS: devenv up did not auto-start start.enable=false process"
