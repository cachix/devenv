{ inputs
, pkgs
, lib
, config
, ...
}:
let
  patchedNix = import ../nix/patched-nix.nix {
    nix = inputs.nix.packages.${pkgs.stdenv.system}.nix;
  };
in
{
  packages = [
    patchedNix.libs.nix-expr-c
    patchedNix.libs.nix-store-c
    patchedNix.libs.nix-util-c
    patchedNix.libs.nix-flake-c
    patchedNix.libs.nix-cmd-c
    patchedNix.libs.nix-fetchers-c
    patchedNix.libs.nix-main-c
    pkgs.boehmgc
    pkgs.rustPlatform.bindgenHook
  ];

  languages.rust.enable = true;
}
