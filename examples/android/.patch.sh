echo "{ pkgs, lib, ... }: {"  > devenv.local.nix
echo "  android.enable = lib.mkForce (pkgs.stdenv.isLinux && !pkgs.stdenv.isAarch64);" >> devenv.local.nix
echo "}" >> devenv.local.nix

