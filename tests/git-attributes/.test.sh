#!/usr/bin/env bash
set -euo pipefail

test_dir=$(mktemp -d)
trap 'rm -rf "$test_dir"' EXIT
cd "$test_dir"
git init -q
# Attributes alone must activate the generated configuration too.
test "$(git check-attr diff -- example.dat)" = 'example.dat: diff: unset'
test ! -e .gitattributes
test ! -e .git/info/attributes
