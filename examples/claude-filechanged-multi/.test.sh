#!/usr/bin/env bash
set -euo pipefail

# Verifies the "|" FileChanged case: a matcher naming several literal files
# at once.
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
[ "$matcher" = ".env|.env.local" ] || fail "matcher should be '.env|.env.local', got '$matcher'"
pass "matcher is preserved verbatim as '.env|.env.local'"

# Building the watch list is plain string-splitting on '|' - no glob or
# regex interpretation. Reproduce that split ourselves and confirm it
# yields exactly the two literal filenames we expect to be watched.
IFS='|' read -r -a segments <<< "$matcher"
[ "${#segments[@]}" -eq 2 ] || fail "expected 2 '|'-separated segments, got ${#segments[@]}"
[ "${segments[0]}" = ".env" ] || fail "first watched file should be '.env', got '${segments[0]}'"
[ "${segments[1]}" = ".env.local" ] || fail "second watched file should be '.env.local', got '${segments[1]}'"
pass "matcher watches exactly two literal files: '${segments[0]}' and '${segments[1]}'"

[ "$(jq -r '.hooks.FileChanged[0].hooks[0].command' "$SETTINGS")" = "echo 'dotenv file changed' >> .claude-filechanged.log" ] \
  || fail "FileChanged hook command mismatch"
pass "command is preserved verbatim"

echo "=== claude-filechanged-multi: all checks passed ==="
