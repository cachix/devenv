#!/usr/bin/env bash
set -euo pipefail

if output=$(devenv shell -- true 2>&1); then
  echo 'Expected directory destination collision to fail evaluation'
  exit 1
fi
echo "$output" | grep -qF 'claude.code.skills.directory: directory destination .claude/skills/directory collides with files: .claude/skills/directory/references/extra.md' || {
  echo "$output"
  exit 1
}
