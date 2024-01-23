{ pkgs, ... }:

{
  languages.javascript.enable = true;
  languages.javascript.npm.install.enable = true;

  processes.remix.exec = "npm run dev";

  services.postgres = {
    enable = true;
    initialDatabases = [{ name = "remix"; }];
    initialScript = ''
      CREATE USER postgres SUPERUSER;
    '';
    listen_addresses = "127.0.0.1";
    port = 5432;
  };

  env = {
    DATABASE_URL = "postgresql://postgres@127.0.0.1:5432/remix";
  };
}
