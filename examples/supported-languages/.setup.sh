# arm is not supported yet
echo "{ pkgs, lib, ... }: { languages.unison.enable = lib.mkForce (!(pkgs.stdenv.isLinux && pkgs.stdenv.isAarch64)); }" > devenv.local.nix