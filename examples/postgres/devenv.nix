{ pkgs, ... }:

{
  packages = [ pkgs.coreutils ];
  services.postgres = {
    enable = true;
    extensions = extensions: [ extensions.postgis ];

    initialDatabases = [{ name = "mydb"; }];

    initialScript = ''
      CREATE EXTENSION IF NOT EXISTS postgis;
    '';
  };
}
