#!/usr/bin/env bash
set -euo pipefail

# `claude.code.agent` names a built-in agent that is not among the
# project-defined agents. `devenv info` should report it as the primary
# agent and list every configured agent as a sub-agent.

out=$(devenv info)

echo "$out" | grep -qF -- "- Primary agent: general-purpose" \
  || { echo "expected '- Primary agent: general-purpose' in devenv info output:"; echo "$out"; exit 1; }

echo "$out" | grep -qF -- "- Sub-agents: code-reviewer, docs-writer, test-writer" \
  || { echo "expected '- Sub-agents: code-reviewer, docs-writer, test-writer' in devenv info output:"; echo "$out"; exit 1; }
