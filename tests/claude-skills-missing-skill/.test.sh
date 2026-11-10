#!/usr/bin/env bash
set -euo pipefail

if output=$(devenv shell -- true 2>&1); then
  echo 'Expected missing SKILL.md to fail evaluation'
  exit 1
fi
echo "$output" | grep -qF 'claude.code.skills.missing: directory-backed skills must be directories containing SKILL.md' || {
  echo "$output"
  exit 1
}
