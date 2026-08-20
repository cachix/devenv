{ pkgs, ... }:

{
  services.cockroachdb = {
    enable = pkgs.stdenv.hostPlatform.isLinux;
  };
}
