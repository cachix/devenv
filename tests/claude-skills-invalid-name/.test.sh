#!/usr/bin/env bash
set -euo pipefail

if output=$(devenv shell -- true 2>&1); then
  echo 'Expected invalid skill name to fail evaluation'
  exit 1
fi
echo "$output" | grep -qF 'claude.code.skills.Invalid_Name: skill names must use lowercase letters' || {
  echo "$output"
  exit 1
}
