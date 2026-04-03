#!/usr/bin/env bash
set -euo pipefail

git_dir=$(git rev-parse --git-dir)
git config core.hooksPath "$(realpath "$git_dir/hooks")"
