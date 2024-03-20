# arm is not supported yet
echo "{ pkgs, lib, ... }: {"  > devenv.local.nix
echo "  languages.unison.enable = lib.mkForce (!(pkgs.stdenv.isLinux && pkgs.stdenv.isAarch64));" >> devenv.local.nix
echo "  languages.standardml.enable = lib.mkForce (!pkgs.stdenv.isAarch64);" >> devenv.local.nix
# https://github.com/NixOS/nixpkgs/issues/297019
echo "  languages.purescript.enable = lib.mkForce (!pkgs.stdenv.isAarch64);" >> devenv.local.nix
echo "}" >> devenv.local.nix
