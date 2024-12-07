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
    setupSchemaScript = ''
      echo "script to run to setup or update database schema. This script must be idempotent."
    '';
  };
}
