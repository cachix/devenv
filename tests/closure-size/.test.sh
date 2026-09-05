#!/usr/bin/env bash
# Ratchet on the size of the devenv closure, which is what dominates the size of
# the container image we publish. The limit is meant to come down over time;
# raising it should be a deliberate decision, not a silent one.
set -euo pipefail

# Darwin's platform closure is larger, so keep a separate ratchet for it.
system=$(nix eval --impure --raw --expr builtins.currentSystem)
case "$system" in
  aarch64-darwin) MAX_MB=532 ;;
  *) MAX_MB=410 ;;
esac

repo=$(cd ../.. && pwd)

echo "Building devenv..."
devenv_path=$(nix build --no-link --print-out-paths "$repo#devenv")

bytes=$(nix path-info --closure-size "$devenv_path" | awk '{print $2}')
mb=$((bytes / 1000 / 1000))

echo "devenv closure on $system: $mb MB (limit $MAX_MB MB)"

if [ "$mb" -gt "$MAX_MB" ]; then
  cat >&2 <<EOF
❌ The devenv closure on $system grew to $mb MB, past its limit of $MAX_MB MB.

See what is taking up the space with:
  nix path-info -rS $repo#devenv | sort -k2 -n | tail -20

A common cause is a flake input without \`nixpkgs.follows\`, which drags a second
copy of glibc, OpenSSL, curl and friends into the closure. Track an offender
back to its source with:
  nix why-depends --precise $repo#devenv <store-path>
EOF
  exit 1
fi

echo "✅ devenv closure is within budget"
