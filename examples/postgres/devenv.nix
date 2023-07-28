{ pkgs, ... }:

{
  packages = [ pkgs.coreutils ];
  services.postgres = {
    enable = true;
    extensions = extensions: [ extensions.postgis ];

    initialDatabases = [{ name = "mydb"; }];

    settings = {
      unix_socket_directories = "/tmp";
    };

    initialScript = ''
      CREATE EXTENSION IF NOT EXISTS postgis;
    '';
  };
}
