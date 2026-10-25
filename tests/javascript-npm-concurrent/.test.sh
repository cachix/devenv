#!/usr/bin/env bash
set -euo pipefail

pids=()
for _ in 1 2 3; do
  devenv shell -- true &
  pids+=($!)
done
for pid in "${pids[@]}"; do
  wait "$pid"
done

installs=$(wc -l < installs.log)
if [ "$installs" -ne 1 ]; then
  echo "expected 1 npm install across concurrent shells, got $installs"
  exit 1
fi

devenv shell -- node -e "require('dep/package.json')"
