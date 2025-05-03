cat > devenv.local.nix << EOF
{ pkgs, lib, ... }: {
  # ARM is not supported.
  languages.unison.enable = lib.mkForce (!(pkgs.stdenv.isLinux && pkgs.stdenv.isAarch64));
  languages.standardml.enable = lib.mkForce (!pkgs.stdenv.isAarch64);
  # https://github.com/NixOS/nixpkgs/issues/297019
  languages.purescript.enable = lib.mkForce (!pkgs.stdenv.isAarch64);
  android.enable = lib.mkForce (pkgs.stdenv.isLinux && !pkgs.stdenv.isAarch64);
  # Doesn't build on macOS or ARM.
  languages.odin.enable = lib.mkForce (!(pkgs.stdenv.isDarwin || (pkgs.stdenv.isLinux && pkgs.stdenv.isAarch64)));
  # macOS is broken.
  languages.racket.enable = lib.mkForce (!pkgs.stdenv.isDarwin);
}
EOF
