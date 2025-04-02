{ pkgs, ... }:

{
  services.keycloak = {
    enable = true;
    port = 8089;
  };

  packages = [ pkgs.curl ];
}
