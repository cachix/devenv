#!/usr/bin/env bash
set -ex
cargo --version
rustc --version

if [[ "$(uname)" == "Darwin" ]]; then
  echo "$RUSTFLAGS" | grep -- "-L framework=$DEVENV_PROFILE/Library/Frameworks"
  echo "$RUSTDOCFLAGS" | grep -- "-L framework=$DEVENV_PROFILE/Library/Frameworks"
  echo "$CFLAGS" | grep -- "-iframework $DEVENV_PROFILE/Library/Frameworks"
fi

[[ "$CARGO_INSTALL_ROOT" == "$DEVENV_STATE/cargo-install" ]]
echo "$PATH" | grep -- "$CARGO_INSTALL_ROOT/bin"

cd app
cargo run
