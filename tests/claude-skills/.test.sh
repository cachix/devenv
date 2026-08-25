#!/usr/bin/env bash
set -euo pipefail

# Skills are written to .claude/skills/<name>/SKILL.md, with any `resources`
# placed alongside them.

skill=.claude/skills/package-scoping/SKILL.md

test -f "$skill" || { echo "missing $skill"; exit 1; }
test -f .claude/skills/minimal/SKILL.md || { echo "missing minimal skill"; exit 1; }

grep -qF -- 'name: package-scoping' "$skill" \
  || { echo "expected name in frontmatter:"; cat "$skill"; exit 1; }

grep -qF -- 'description: "Decide system vs user: use when a \"package\" lands in the wrong layer."' "$skill" \
  || { echo "expected description in frontmatter:"; cat "$skill"; exit 1; }

grep -qF -- 'allowed-tools: "Read, Grep, Bash(git status: *)"' "$skill" \
  || { echo "expected allowed-tools in frontmatter:"; cat "$skill"; exit 1; }

grep -qF -- '# Package scoping' "$skill" \
  || { echo "expected skill body:"; cat "$skill"; exit 1; }

# No tool restriction means no allowed-tools key, and no blank line left where
# it would have gone.
if grep -q 'allowed-tools' .claude/skills/minimal/SKILL.md; then
  echo "minimal skill should not declare allowed-tools:"
  cat .claude/skills/minimal/SKILL.md
  exit 1
fi

frontmatter=$(awk 'NR == 1 && $0 == "---" { inside = 1; next } inside && $0 == "---" { exit } inside' .claude/skills/minimal/SKILL.md)
if [ -n "$(echo "$frontmatter" | grep '^$' || true)" ]; then
  echo "frontmatter should not contain blank lines:"
  cat .claude/skills/minimal/SKILL.md
  exit 1
fi

# Bundled resources land next to SKILL.md.
grep -qF -- '/etc/profiles/per-user' .claude/skills/package-scoping/references/table.md \
  || { echo "missing bundled resource"; exit 1; }

# A resource marked executable is runnable.
test -x .claude/skills/package-scoping/scripts/check.sh \
  || { echo "bundled script is not executable:"; ls -l .claude/skills/package-scoping/scripts/check.sh; exit 1; }

[ "$(.claude/skills/package-scoping/scripts/check.sh)" = "SCRIPT-RESOURCE-RAN" ] \
  || { echo "bundled script did not run"; exit 1; }

# The plain-path shorthand stays non-executable.
if [ -x .claude/skills/package-scoping/references/table.md ]; then
  echo "plain-path resource should not be executable"
  exit 1
fi

# Skills are reported by `devenv info`.
devenv info | grep -qF -- "- Skills: minimal, package-scoping" \
  || { echo "expected skills in devenv info output:"; devenv info; exit 1; }
