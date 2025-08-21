#!/usr/bin/env bash
set -ex
cargo --version
rustc --version

[[ "$CARGO_INSTALL_ROOT" == "$DEVENV_STATE/cargo-install" ]]
echo "$PATH" | grep -- "$CARGO_INSTALL_ROOT/bin"

# Test the original cargo workflow
cd app
cargo run

# Test the crate2nix import functionality
cd ..
echo "Testing crate2nix import..."

# The myapp package should be available as a derivation
devenv build outputs.myapp

# Verify the package can be built
if command -v app &> /dev/null; then
    echo "crate2nix imported package 'app' is available in PATH"
    app
else
    echo "Note: app binary not in PATH during devenv shell"
fi

echo "crate2nix import test completed successfully!"
