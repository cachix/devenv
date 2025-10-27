#!/usr/bin/env bash

set -ex

# Set up nixpkgs
export NIX_PATH='nixpkgs=https://github.com/cachix/devenv-nixpkgs/archive/rolling.tar.gz'

nix-shell -p npins --run "npins init && npins add github cachix devenv"

# Verify that we've entered the shell
nix-shell --command 'printenv IN_NON_FLAKE_SHELL'
