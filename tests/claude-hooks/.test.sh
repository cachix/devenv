#!/usr/bin/env bash
set -euo pipefail

# Checks the claude.code.hooks module output directly against the generated
# .claude/settings.json - no `claude` binary or live session is involved.

SETTINGS=".claude/settings.json"

fail() {
  echo "FAIL: $1" >&2
  echo "Contents of $SETTINGS:" >&2
  cat "$SETTINGS" >&2
  exit 1
}

pass() {
  echo "ok   $1"
}

test -f "$SETTINGS" || { echo "$SETTINGS does not exist" >&2; exit 1; }

# FileChanged: the hook is grouped under a "FileChanged" key (not PostToolUse
# or any tool-related event), keeps its glob matcher verbatim, and - since no
# timeout was set - carries no "timeout" field at all.
[ "$(jq -r '.hooks.FileChanged[0].matcher' "$SETTINGS")" = ".envrc" ] \
  || fail "FileChanged hook matcher should be '.envrc'"
[ "$(jq -r '.hooks.FileChanged[0].hooks[0].command' "$SETTINGS")" = "direnv reload" ] \
  || fail "FileChanged hook command mismatch"
[ "$(jq -r '.hooks.FileChanged[0].hooks[0].timeout // "absent"' "$SETTINGS")" = "absent" ] \
  || fail "FileChanged hook should have no timeout field when unset"
pass "FileChanged hook is generated under hooks.FileChanged with its glob matcher and no timeout"

# PostToolUse hook with an explicit timeout: the numeric value is passed through.
[ "$(jq -r '.hooks.PostToolUse[0].hooks[0].timeout' "$SETTINGS")" = "120" ] \
  || fail "PostToolUse hook should have timeout=120"
pass "PostToolUse hook carries its explicit timeout=120"

# Stop hook without a timeout: field is omitted entirely, not null or 0.
[ "$(jq -r '.hooks.Stop[0].hooks[0].timeout // "absent"' "$SETTINGS")" = "absent" ] \
  || fail "Stop hook should have no timeout field when unset"
pass "Stop hook has no timeout field when unset"

echo "=== claude-hooks: all checks passed ==="
