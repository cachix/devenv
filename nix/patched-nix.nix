{ nix }:

nix.appendPatches [
  ./nix-build-environment-ca-derivations.patch
]
