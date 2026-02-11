#!/usr/bin/env bash
set -xe
set -o pipefail

# Test devenv integrated into bare Nix flake
nix flake init --template "${DEVENV_REPO}"
nix flake update --override-input devenv "${DEVENV_REPO}"

# Test that nix develop works with --no-pure-eval
nix develop --accept-flake-config --no-pure-eval --command echo nix-develop started successfully 2>&1 | tee ./console
grep -F 'nix-develop started successfully' <./console
grep -F 'Hello, world!' <./console

# Assert that nix-develop fails in pure mode
if nix develop --command echo nix-develop started in pure mode 2>&1 | tee ./console
then
  echo "nix-develop was able to start in pure mode. This is explicitly not supported."
  exit 1
fi
grep -F 'devenv was not able to determine the current directory.' <./console
