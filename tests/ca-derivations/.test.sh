#!/usr/bin/env bash
set -euo pipefail

# Regression test for #2364. BuildEnvironment used to leave Nix's deferred
# output placeholder in `out`, which derivation validation rejects.
NIX_CONFIG=$'experimental-features = nix-command flakes ca-derivations\nextra-experimental-features =' \
  devenv shell -- true
