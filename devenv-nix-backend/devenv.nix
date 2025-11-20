{ inputs
, pkgs
, lib
, config
, ...
}:
{
  packages = [
    inputs.nix.packages.${pkgs.stdenv.system}.nix-expr-c
    inputs.nix.packages.${pkgs.stdenv.system}.nix-store-c
    inputs.nix.packages.${pkgs.stdenv.system}.nix-util-c
    inputs.nix.packages.${pkgs.stdenv.system}.nix-flake-c
    inputs.nix.packages.${pkgs.stdenv.system}.nix-cmd-c
    inputs.nix.packages.${pkgs.stdenv.system}.nix-fetchers-c
    pkgs.boehmgc
    pkgs.rustPlatform.bindgenHook
  ];

  languages.rust.enable = true;
}
