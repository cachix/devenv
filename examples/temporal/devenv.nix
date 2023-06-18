{ pkgs, ... }:
{
  services.temporal = {
    enable = true;

    port = 17233;

    namespaces = [ "mynamespace" ];

    state = {
      ephemeral = false;
      sqlite-pragma = {
        journal_mode = "wal";
      };
    };
  };
}
