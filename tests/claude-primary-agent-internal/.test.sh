#!/usr/bin/env bash
set -euo pipefail

# `claude.code.agent` names one of the project-defined agents. `devenv info`
# should report it as the primary agent and list the *other* configured
# agents as sub-agents, excluding the primary one.

out=$(devenv info)

echo "$out" | grep -qF -- "- Primary agent: code-reviewer" \
  || { echo "expected '- Primary agent: code-reviewer' in devenv info output:"; echo "$out"; exit 1; }

echo "$out" | grep -qF -- "- Sub-agents: docs-writer, test-writer" \
  || { echo "expected '- Sub-agents: docs-writer, test-writer' in devenv info output:"; echo "$out"; exit 1; }

if echo "$out" | grep -qF -- "code-reviewer, docs-writer"; then
  echo "primary agent 'code-reviewer' must not be listed among sub-agents:"
  echo "$out"
  exit 1
fi
