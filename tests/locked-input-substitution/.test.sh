#!/usr/bin/env bash
set -euo pipefail

fixture=$(mktemp -d)
cache_dir="$DEVENV_STATE/locked-input-cache"
cache_url=${CACHE_URL:?CACHE_URL is not set}
trap 'rm -rf "$fixture" "$cache_dir"' EXIT
mkdir "$fixture/source" "$fixture/project"
cat > "$fixture/source/devenv.nix" <<'EOF'
{
  env.CACHED_INPUT = "from binary cache";
}
EOF
# Give this test its own source path, so deleting it cannot affect other tests.
printf '%s\n' "$fixture" > "$fixture/source/nonce"
source_path=$(nix --extra-experimental-features nix-command store add-path --name source "$fixture/source")
nar_hash=$(nix --extra-experimental-features nix-command hash path "$fixture/source")
nix --extra-experimental-features nix-command copy --to "file://$cache_dir" "$source_path"
nix-store --delete "$source_path"
if nix --extra-experimental-features nix-command path-info "$source_path"; then
    echo "Test source unexpectedly remains in the local store" >&2
    exit 1
fi

# The origin never exists. Only the binary cache can supply the source.
origin="file://$fixture/unreachable.tar.gz"
encoded_hash=$(jq -rn --arg hash "$nar_hash" '$hash | @uri')
cat > "$fixture/project/devenv.yaml" <<EOF
inputs:
  cached:
    url: "tarball+$origin?narHash=$encoded_hash&lastModified=1000000000"
    flake: false
imports:
  - cached
EOF
cat > "$fixture/project/devenv.nix" <<'EOF'
{ ... }:
assert builtins ? fetchFinalTree;
{ }
EOF
jq --arg url "$origin" --arg hash "$nar_hash" '
  .nodes[.root].inputs.cached = "cached"
  | .nodes.cached = {
      flake: false,
      locked: { type: "tarball", url: $url, narHash: $hash, lastModified: 1000000000 },
      original: { type: "tarball", url: $url, narHash: $hash, lastModified: 1000000000 }
    }
' devenv.lock > "$fixture/project/devenv.lock"

cd "$fixture/project"
# shellcheck disable=SC2016
devenv --no-eval-cache --nix-option extra-substituters "$cache_url" shell bash -- -c \
    'test "$CACHED_INPUT" = "from binary cache"'
nix-store --check-validity "$source_path"

# With the cache removed, a fresh evaluation must reuse the local store copy.
mv "$cache_dir" "$fixture/cache-unavailable"
# shellcheck disable=SC2016
devenv --no-eval-cache shell bash -- -c \
    'test "$CACHED_INPUT" = "from binary cache"'

# Incomplete lock metadata must still be supplied by the ordinary fetcher,
# even when the source is already present in the store.
touch -d '@1000000000' "$fixture/source" "$fixture/source/"*
tar -czf "$fixture/metadata.tar.gz" -C "$fixture" source
cat > devenv.yaml <<EOF
inputs:
  cached:
    url: "tarball+file://$fixture/metadata.tar.gz?narHash=$encoded_hash"
    flake: false
imports:
  - cached
EOF
cat > devenv.nix <<'EOF'
{ inputs, ... }:
{
  env.INPUT_LAST_MODIFIED = toString inputs.cached.lastModified;
}
EOF
jq --arg url "file://$fixture/metadata.tar.gz" '
  .nodes.cached.locked.url = $url
  | .nodes.cached.original.url = $url
  | del(.nodes.cached.locked.lastModified, .nodes.cached.original.lastModified)
' devenv.lock > next.lock
mv next.lock devenv.lock
# shellcheck disable=SC2016
devenv --no-eval-cache shell bash -- -c \
    'test "$INPUT_LAST_MODIFIED" = 1000000000'
