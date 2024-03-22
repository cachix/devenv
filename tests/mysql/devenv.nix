{ pkgs, ... }:
{
  services.mysql = {
    enable = true;
    initialDatabases = [{ name = "db"; }];
    ensureUsers = [{
      name = "db";
      password = "db";
      ensurePermissions = { "*.*" = "ALL PRIVILEGES"; };
    }];
    settings = {
      mysql = {
        host = "127.0.0.1";
        user = "db";
        password = "db";
      };
    };
  };
}
