#!/usr/bin/env bash
set -eu -o pipefail

# Test CLI options feature
cd "$(dirname "$0")"

# Run devenv info with the options and capture output
OUTPUT=$(devenv --option languages.rust.version 1.23 --option services.redis.enable true info)

# Check for expected RUST_VERSION
echo "$OUTPUT" | grep -E "RUST_VERSION:.*1.23" || {
  echo "ERROR: Expected CLI option override for RUST_VERSION to be '1.23'"
  exit 1
}

# Check for expected REDIS_ENABLED
echo "$OUTPUT" | grep -E "REDIS_ENABLED:.*true" || {
  echo "ERROR: Expected CLI option override for REDIS_ENABLED to be 'true'"
  exit 1
}


# Test Nix expression evaluation
OUTPUT=$(devenv --option languages.rust.version \"builtins.toString (1 + 1)\" info)

echo "$OUTPUT" | grep -E "RUST_VERSION:.*2" || {
  echo "ERROR: Expected Nix expression evaluation for RUST_VERSION to be '2'"
  exit 1
}


# Test string values
OUTPUT=$(eval "devenv --option languages.rust.version '\"custom-version\"' info")

echo "$OUTPUT" | grep -E "RUST_VERSION:.*custom-version" || {
  echo "ERROR: Expected quoted string for RUST_VERSION to be 'custom-version'"
  exit 1
}
