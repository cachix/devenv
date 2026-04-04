#!/usr/bin/env bash
set -euo pipefail

mkdir -p custom-hooks
git config core.hooksPath "$PWD/custom-hooks"
