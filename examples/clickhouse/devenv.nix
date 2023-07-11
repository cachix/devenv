{ pkgs, ... }:

{
  services.clickhouse = {
    enable = true;
    config = ''
      http_port: 8123
      listen_host: 127.0.0.1
    '';
  };
}
