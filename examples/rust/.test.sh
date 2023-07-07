#!/bin/sh
set -ex
cargo --version
rustc --version

echo "$RUSTFLAGS" | grep -- "--jobs 1"

if [[ "$(uname)" -eq "Darwin" ]] then
  echo "$RUSTFLAGS" | grep -- "-L framework=$DEVENV_PROFILE/Library/Frameworks --jobs 1"
  echo "$RUSTDOCFLAGS" | grep -- "-L framework=$DEVENV_PROFILE/Library/Frameworks"
  echo "$CFLAGS" | grep -- "-iframework $DEVENV_PROFILE/Library/Frameworks"
fi
