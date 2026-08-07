#!/usr/bin/env bash
set -euo pipefail

# No agents are configured, and `claude.code.agent` points at a built-in
# Claude Code agent that isn't part of this project. `devenv info` should
# report it as the primary agent and print no "Sub-agents" line.

out=$(devenv info)

echo "$out" | grep -qF -- "- Primary agent: general-purpose" \
  || { echo "expected '- Primary agent: general-purpose' in devenv info output:"; echo "$out"; exit 1; }

if echo "$out" | grep -q -- "- Sub-agents:"; then
  echo "did not expect a '- Sub-agents:' line in devenv info output:"
  echo "$out"
  exit 1
fi
