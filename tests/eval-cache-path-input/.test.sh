#!/usr/bin/env bash
set -ex

# Verifies that editing files inside a local path: input invalidates the
# evaluation cache instead of serving a stale store copy.

cd project

assert_greet() {
    local expected=$1
    local actual
    actual=$(devenv print-dev-env | grep -o "GREET='[a-z]*'")
    if [ "$actual" != "GREET='$expected'" ]; then
        echo "ERROR: expected GREET='$expected', got $actual"
        exit 1
    fi
}

assert_source_metadata() {
    local metadata_env
    local last_modified
    set +x
    metadata_env=$(devenv print-dev-env)
    set -x

    if ! grep -q "INPUT_NAR_HASH='sha256-" <<<"$metadata_env"; then
        echo "ERROR: live path flake did not expose self.narHash"
        exit 1
    fi

    last_modified=$(grep -o "INPUT_LAST_MODIFIED='[0-9]*'" <<<"$metadata_env" | grep -o '[0-9]*')
    if [ -z "$last_modified" ] || [ "$last_modified" = 0 ]; then
        echo "ERROR: live path flake exposed an invalid self.lastModified"
        exit 1
    fi
}

# Initial evaluation populates the cache
assert_greet one
assert_source_metadata

# The input's files must be tracked so direnv and the cache see changes
if ! grep -q "config-repo" .devenv/input-paths.txt; then
    echo "ERROR: config-repo files are not tracked in input-paths.txt"
    cat .devenv/input-paths.txt
    exit 1
fi

# Editing a file inside the path: input must invalidate the cache
sed -i.bak 's/one/two/' ../config-repo/devenv.nix
assert_greet two

# A devenv.local.nix appearing in the path: input must also invalidate
cat > ../config-repo/devenv.local.nix <<'EOF'
{ lib, ... }:
{
  env.GREET = lib.mkForce "three";
}
EOF
assert_greet three

echo "Test completed successfully!"
