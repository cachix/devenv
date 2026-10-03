#!/usr/bin/env bash
# SecretSpec `as_path` secrets must point at files that exist for as long as
# whatever reads the exported path runs.
set -euo pipefail

# direnv loads print-dev-env's output after devenv has exited, so the file has
# to outlive devenv.
devenv print-dev-env > env.sh
path=$(sed -n "s/^TLS_KEY_PATH='\(.*\)'$/\1/p" env.sh)
test -n "$path"
test "$(cat "$path")" = "test-key"
echo "✓ print-dev-env leaves $path in place"
