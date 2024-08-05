#!/usr/bin/env bash
set -ex
cargo --version
rustc --version

[[ "$CARGO_INSTALL_ROOT" == "$DEVENV_STATE/cargo-install" ]]
echo "$PATH" | grep -- "$CARGO_INSTALL_ROOT/bin"

cd app
cargo run
