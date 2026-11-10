#!/usr/bin/env bash
set -euo pipefail

if output=$(devenv shell -- true 2>&1); then
  echo 'Expected exact skill destination collision to fail evaluation'
  exit 1
fi
echo "$output" | grep -qF 'claude.code.skills.example: directory destination .claude/skills/example collides with files: .claude/skills/example' || {
  echo "$output"
  exit 1
}
