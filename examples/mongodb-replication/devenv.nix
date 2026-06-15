{ pkgs, config, ... }:

{
  services.mongodb = {
    enable = true;
    replication = {
      enable = true;
      numNodes = 3;
    };
    initDatabaseUsername = "mongouser";
    initDatabasePassword = "secret";
  };
}
