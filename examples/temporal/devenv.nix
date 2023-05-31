{ pkgs, ... }:
{
  services.temporal = {
    enable = true;

    namespaces = [ "mynamespace" ];

    state = {
      ephemeral = false;
      sqlite-pragma = {
        journal_mode = "wal";
      };
    };
  };
}
