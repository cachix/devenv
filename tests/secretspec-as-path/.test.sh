#!/usr/bin/env bash
# direnv reads these paths after print-dev-env exits.
set -euo pipefail

load_key_path() {
  devenv "$@" print-dev-env > env.sh
  sed -n "s/^TLS_KEY_PATH='\(.*\)'$/\1/p" env.sh
}

path=$(load_key_path)
test -n "$path"
test "$(cat "$path")" = "test-key"

# Another profile must neither overwrite nor remove the first shell's key.
printf 'TLS_KEY=production-key\n' > .env
production_path=$(load_key_path --secretspec-profile production)
test "$path" != "$production_path"
test "$(cat "$path")" = "test-key"
test "$(cat "$production_path")" = "production-key"

# Different stores of the same provider also need independent paths.
printf 'TLS_KEY=other-store-key\n' > .env.other
other_path=$(load_key_path --secretspec-provider dotenv:.env.other)
test "$path" != "$other_path"
test "$(cat "$path")" = "test-key"
test "$(cat "$other_path")" = "other-store-key"

# Disabling the integration removes files from every context.
printf 'secretspec:\n  enable: false\n' > devenv.yaml
devenv print-dev-env > env.sh
test ! -e "$path"
test ! -e "$production_path"
test ! -e "$other_path"

# Removing the manifest also revokes previously exported paths.
printf 'secretspec:\n  enable: true\n  provider: dotenv\n' > devenv.yaml
path=$(load_key_path)
test -f "$path"
rm secretspec.toml
devenv print-dev-env > env.sh
test ! -e "$path"
echo "✓ SecretSpec runtime files are isolated and cleaned up"
