#!/bin/sh
set -ex
cargo --version
rustc --version

echo "$RUSTFLAGS" | grep -- "--verbose"

if [[ "$(uname)" == "Darwin" ]] then
  echo "$RUSTFLAGS" | grep -- "-L framework=$DEVENV_PROFILE/Library/Frameworks --verbose"
  echo "$RUSTDOCFLAGS" | grep -- "-L framework=$DEVENV_PROFILE/Library/Frameworks"
  echo "$CFLAGS" | grep -- "-iframework $DEVENV_PROFILE/Library/Frameworks"
fi

cd app
cargo run
