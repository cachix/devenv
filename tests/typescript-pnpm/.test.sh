#!/usr/bin/env bash
set -euo pipefail

node --version
pnpm --version
test "$(node --version)" = "$(pnpm exec node --version)"
tsc --version
node -e "require('local-dependency/package.json')"
test -f node_modules/pnpm-lock.yaml.checksum
