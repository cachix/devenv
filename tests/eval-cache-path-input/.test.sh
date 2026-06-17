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

# Initial evaluation populates the cache
assert_greet one

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
