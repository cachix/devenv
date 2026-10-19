#!/usr/bin/env bash
set -euo pipefail

test_dir=$(mktemp -d)
trap 'rm -rf "$test_dir"' EXIT
cd "$test_dir"
git init -q
export GIT_CONFIG_GLOBAL="$test_dir/global-config"
printf '[merge]\n tool = custom\n' > "$GIT_CONFIG_GLOBAL"
test "$(git config merge.tool)" = custom
test ! -e .gitattributes
