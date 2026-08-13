{ config, pkgs, ... }:

{
  services.clickhouse = {
    enable = true;
    config = ''
      # http_port: 8123
    '';
  };

  enterTest = ''
    export CLICKHOUSE_PORT=${toString config.processes.clickhouse-server.ports.main.value}
  '';
}
