{ pkgs, ... }:
{
  packages = [
    pkgs.nix
    pkgs.gnutar
    pkgs.gzip
  ];
}
