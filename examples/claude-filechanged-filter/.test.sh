#!/usr/bin/env bash
set -euo pipefail

# Verifies the "filtering matcher" FileChanged case: two hook entries whose
# watched files overlap, demonstrating that a matcher does two different
# jobs (per https://code.claude.com/docs/en/hooks#filechanged):
#
#   1. Building the watch list: split on '|', each segment a literal
#      filename - this is a per-project union across every FileChanged
#      entry, so both hooks below contribute to one combined watch list.
#   2. Filtering which hooks run: once some watched file changes, each
#      entry's OWN matcher is re-evaluated - this time as a standard
#      matcher (exact string, or regex when it contains characters outside
#      [A-Za-z0-9_|]) - against the *basename* of the changed file, to
#      decide whether that entry's command fires for that particular file.
#
# Part 1 below checks the real .claude/settings.json devenv.nix generates.
# Part 2 re-implements rule (2) verbatim from the docs and walks it against
# a small table of changed-file basenames, to make the filtering behavior
# concrete without needing a live `claude` session (which would require
# credentials and isn't deterministic for CI).

SETTINGS=".claude/settings.json"

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

pass() {
  echo "ok   $1"
}

test -f "$SETTINGS" || { echo "$SETTINGS does not exist" >&2; exit 1; }

echo "--- part 1: generated settings.json ---"

# notify-env-change sorts before warn-production-env (hooks are emitted in
# attribute-name order), so it's index 0 and warn-production-env is index 1.
broad_matcher=$(jq -r '.hooks.FileChanged[0].matcher' "$SETTINGS")
narrow_matcher=$(jq -r '.hooks.FileChanged[1].matcher' "$SETTINGS")

[ "$broad_matcher" = ".env|.env.local|.env.production" ] \
  || fail "broad matcher should be '.env|.env.local|.env.production', got '$broad_matcher'"
pass "notify-env-change keeps its broad matcher verbatim"

[ "$narrow_matcher" = ".env.production" ] \
  || fail "narrow matcher should be '.env.production', got '$narrow_matcher'"
pass "warn-production-env keeps its narrow matcher verbatim"

[ "$(jq -r '.hooks.FileChanged | length' "$SETTINGS")" = "2" ] \
  || fail "expected exactly 2 FileChanged hook entries, devenv must not merge or drop overlapping matchers"
pass "both FileChanged entries are emitted independently (devenv does not merge overlapping matchers)"

echo "--- part 2: reference filtering behavior (from the docs, not a live session) ---"

# Reimplements the Claude Code "standard matcher rules", applied to a
# matcher and a changed file's basename:
#   - "*" or "" matches everything.
#   - A value made only of [A-Za-z0-9_|] is an exact string, or a list of
#     exact strings separated by '|'.
#   - Anything else is evaluated as an unanchored regular expression.
# (FileChanged/StopFailure use this narrower exact-match charset - no
# hyphen/space/comma - but neither matcher here contains those anyway.)
hook_fires_for() {
  local matcher="$1" basename="$2"
  if [ "$matcher" = "*" ] || [ -z "$matcher" ]; then
    return 0
  fi
  if [[ "$matcher" =~ ^[A-Za-z0-9_\|]+$ ]]; then
    local seg
    IFS='|' read -r -a segs <<< "$matcher"
    for seg in "${segs[@]}"; do
      [ "$seg" = "$basename" ] && return 0
    done
    return 1
  fi
  grep -Eq -- "$matcher" <<< "$basename"
}

check_case() {
  local basename="$1" want_broad="$2" want_narrow="$3"

  local got_broad=no got_narrow=no
  hook_fires_for "$broad_matcher" "$basename" && got_broad=yes
  hook_fires_for "$narrow_matcher" "$basename" && got_narrow=yes

  [ "$got_broad" = "$want_broad" ] \
    || fail "notify-env-change: expected fire=$want_broad for '$basename', got fire=$got_broad"
  [ "$got_narrow" = "$want_narrow" ] \
    || fail "warn-production-env: expected fire=$want_narrow for '$basename', got fire=$got_narrow"
  pass "'$basename' changed -> notify-env-change=$got_broad, warn-production-env=$got_narrow"
}

# Both hooks watch (contribute to the union watch list) all three env
# files, but only fire per-file according to their own matcher:
check_case ".env"            yes no   # only the broad hook covers plain .env
check_case ".env.local"      yes no   # only the broad hook covers .env.local
check_case ".env.production" yes yes  # both hooks fire - this is the sensitive file
check_case "unrelated.txt"   no  no   # not in either watch list - neither fires

echo "=== claude-filechanged-filter: all checks passed ==="
