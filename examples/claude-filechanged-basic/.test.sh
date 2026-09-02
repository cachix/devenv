#!/usr/bin/env bash
set -euo pipefail

# Verifies the "base" FileChanged case: a matcher that names a single file.
#
# This checks the real .claude/settings.json that devenv.nix generates, not
# a live Claude Code session - no `claude` binary or credentials involved,
# so it stays fast and deterministic in CI.

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

matcher=$(jq -r '.hooks.FileChanged[0].matcher' "$SETTINGS")
[ "$matcher" = ".envrc" ] || fail "matcher should be the literal string '.envrc', got '$matcher'"
pass "matcher is preserved verbatim as '.envrc'"

# Per the Claude Code docs, building the watch list for FileChanged never
# expands globs or regexes - a matcher is split on '|' and each segment is
# registered as a literal filename. A matcher with no '|' at all, like this
# one, therefore watches exactly one literal file: itself.
[[ "$matcher" != *"|"* ]] || fail "this example's matcher must not contain '|' (that's the multi-file case)"
pass "single-segment matcher names exactly one literal file to watch: '$matcher'"

[ "$(jq -r '.hooks.FileChanged[0].hooks[0].command' "$SETTINGS")" = "direnv reload" ] \
  || fail "FileChanged hook command mismatch"
pass "command is 'direnv reload'"

echo "=== claude-filechanged-basic: all checks passed ==="
