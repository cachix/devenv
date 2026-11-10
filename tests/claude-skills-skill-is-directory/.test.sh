#!/usr/bin/env bash
set -euo pipefail

if output=$(devenv shell -- true 2>&1); then
  echo 'Expected a directory named SKILL.md to fail evaluation'
  exit 1
fi
echo "$output" | grep -qF 'claude.code.skills.example: directory-backed skills must be directories containing a regular SKILL.md file' || {
  echo "$output"
  exit 1
}
