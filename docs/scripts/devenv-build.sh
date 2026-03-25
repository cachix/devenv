#!/usr/bin/env bash

# Helper: runs `devenv build <attr>` and returns the output path.
# devenv 2.0.0+ outputs JSON, older versions output the path directly.

set -euo pipefail

attr="$1"

devenv_version=$(devenv version | sed -n 's/devenv \([0-9]*\.[0-9]*\.[0-9]*\).*/\1/p')

if printf '%s\n' "2.0.0" "$devenv_version" | sort -V | head -n1 | grep -q "2.0.0"; then
	devenv build "$attr" | jq -r ".\"$attr\""
else
	devenv build "$attr"
fi
