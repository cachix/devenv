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

# `devenv shell -- <cmd>` used to exec the command after deleting the file.
# Now the file exists while the command runs and is deleted when it exits.
path=$(devenv shell -- bash -c 'test "$(cat "$TLS_KEY_PATH")" = "test-key" && echo "$TLS_KEY_PATH"' | tail -n1)
test -n "$path"
test ! -e "$path"
echo "✓ devenv shell -- <cmd> read $path, then removed it"

# The command's exit status passes through.
status=0
devenv shell -- bash -c 'exit 7' || status=$?
test "$status" = 7
echo "✓ devenv shell -- <cmd> exits with the command's status"

# `devenv up -d` hands the file to the detached process manager, which reads
# it after this client exits and removes it when it stops.
devenv up -d
devenv processes wait
path=$(cat watched-path)
test "$(cat "$path")" = "test-key"
devenv processes down
test ! -e "$path"
echo "✓ devenv up -d kept $path for the manager, which removed it on stop"
