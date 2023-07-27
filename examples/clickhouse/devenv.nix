{ pkgs, ... }:

{
  services.clickhouse = {
    enable = true;
    config = ''
      # http_port: 8123
    '';
  };
}
