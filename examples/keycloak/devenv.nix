{ pkgs, ... }:

{
  services.keycloak = {
    enable = true;
    settings.http-port = 8089;

    realmExport = {
      master = { };
    };
  };

  packages = [ pkgs.curl ];
}
