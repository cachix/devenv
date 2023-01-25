{ pkgs, config, ... }:

{
  services.mysql.enable = true;
  # enable for mariadb
  # services.mysql.package = pkgs.mariadb;
  services.mysql.initialDatabases = [{ name = "test_database"; }];
  services.mysql.ensureUsers = [
    {
      name = "test_database";
      password = "test_database";
      ensurePermissions = { "test_database.*" = "ALL PRIVILEGES"; };
    }
  ];
}
