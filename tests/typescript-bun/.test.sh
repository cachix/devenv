#!/usr/bin/env bash
set -euo pipefail

bun --version
tsc --version
typescript-language-server --version
test "$(bun frontend/main.ts)" = "TypeScript with Bun"
test -f frontend/node_modules/bun.lock.checksum

case ":$PATH:" in
  *":$DEVENV_ROOT/frontend/node_modules/.bin:"*) ;;
  *) echo "Missing absolute node_modules/.bin entry: $PATH" >&2; exit 1 ;;
esac

# Local executables must remain accessible after changing directories.
cd frontend/tool
test "$(local-tool)" = "local dependency"
