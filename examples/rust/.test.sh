set -ex
cargo --version
rustc --version

if [[ "$(uname)" == "Darwin" ]]; then
  echo "$RUSTFLAGS" | grep -- "-L framework=$DEVENV_PROFILE/Library/Frameworks"
  echo "$RUSTDOCFLAGS" | grep -- "-L framework=$DEVENV_PROFILE/Library/Frameworks"
  echo "$CFLAGS" | grep -- "-iframework $DEVENV_PROFILE/Library/Frameworks"
fi

cd app
cargo run
