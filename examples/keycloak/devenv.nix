{ pkgs, ... }:

{
  services.keycloak.enable = true;

  packages = [ pkgs.coreutils ];
}
