cat >devenv.local.nix <<EOF
{ pkgs, lib, ... }: {
  # ARM is not supported.
  languages.unison.enable = lib.mkForce (!(pkgs.stdenv.hostPlatform.isLinux && pkgs.stdenv.hostPlatform.isAarch64));
  languages.standardml.enable = lib.mkForce (!pkgs.stdenv.hostPlatform.isAarch64);
  # https://github.com/NixOS/nixpkgs/issues/297019
  languages.purescript.enable = lib.mkForce (!pkgs.stdenv.hostPlatform.isAarch64);
  android.enable = lib.mkForce (pkgs.stdenv.hostPlatform.isLinux && !pkgs.stdenv.hostPlatform.isAarch64);
  # Doesn't build on macOS or ARM.
  languages.odin.enable = lib.mkForce (!(pkgs.stdenv.hostPlatform.isDarwin || (pkgs.stdenv.hostPlatform.isLinux && pkgs.stdenv.hostPlatform.isAarch64)));
  # macOS is broken.
  languages.racket.enable = lib.mkForce (!pkgs.stdenv.hostPlatform.isDarwin);
  # Swift broken on Linux with GCC 14 - https://github.com/NixOS/nixpkgs/pull/468796
  languages.swift.enable = lib.mkForce pkgs.stdenv.hostPlatform.isDarwin;
  # lobster is marked broken on macOS
  languages.lobster.enable = lib.mkForce (!pkgs.stdenv.hostPlatform.isDarwin);
}
EOF
