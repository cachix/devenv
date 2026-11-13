#!/usr/bin/env bash
set -euo pipefail

deno --version
node --version
tsc --version
typescript-language-server --version
test "$DENO_INSTALL_ROOT" = "$DEVENV_STATE/deno"
test "$DENO_DIR" = "$DENO_INSTALL_ROOT/cache"
test "$(deno eval 'const value: string = "TypeScript with Deno"; console.log(value)')" = "TypeScript with Deno"

case ":$PATH:" in
  *":$DENO_INSTALL_ROOT/bin:"*) ;;
  *) echo "Missing Deno install directory on PATH" >&2; exit 1 ;;
esac
