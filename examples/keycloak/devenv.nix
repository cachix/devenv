{ pkgs, ... }:

{
  services.keycloak = {
    enable = true;
    settings.http-port = 8089;
  };

  packages = [ pkgs.curl ];
}
