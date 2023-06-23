{ pkgs, config, ... }:

{
  services.mongodb = {
    enable = true;
    initDatabaseUsername = "mongouser";
    initDatabasePassword = "secret";
  };
}
