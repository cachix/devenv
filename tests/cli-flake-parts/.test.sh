#!/usr/bin/env bash
set -xe
set -o pipefail

# Test devenv integrated into flake-parts Nix flake
nix flake init --template "${DEVENV_REPO}#flake-parts"
nix flake update --override-input devenv "${DEVENV_REPO}"

# Test that nix develop works with the devenv-root override
nix develop --accept-flake-config --override-input devenv-root "file+file://"<(printf %s "$PWD") --command echo nix-develop started successfully |& tee ./console
grep -F 'nix-develop started successfully' <./console
grep -F 'Hello, world!' <./console

# Test that a container can be built (Linux only)
if [ "$(uname)" = "Linux" ]
then
  nix build --override-input devenv-root "file+file://"<(printf %s "$PWD") --accept-flake-config --show-trace .#container-processes
fi
