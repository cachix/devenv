{ pkgs, ... }:
{
  services.temporal = {
    enable = true;

    namespaces = [ "mynamespace" ];

    ephemeral = false;
  };
}
