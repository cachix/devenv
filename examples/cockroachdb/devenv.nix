{ pkgs, ... }:

{
  services.cockroachdb = {
    enable = pkgs.stdenv.isLinux;
  };
}
