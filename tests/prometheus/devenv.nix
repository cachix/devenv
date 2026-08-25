{
  config,
  pkgs,
  lib,
  ...
}:
let
  prometheusPort = config.processes.prometheus.ports.main.value;
in
{
  services.prometheus = {
    enable = true;
    port = 9090;
    scrapeConfigs = [
      {
        job_name = "prometheus";
        static_configs = [
          {
            targets = [ "127.0.0.1:${toString prometheusPort}" ];
          }
        ];
      }
    ];
    globalConfig = {
      scrape_interval = "1s"; # Short interval for quick testing
      evaluation_interval = "1s";
    };
  };

  scripts.ping-prometheus.exec = ''
    ${lib.getExe pkgs.curl} -sf http://127.0.0.1:${toString prometheusPort}/-/healthy
  '';

  enterTest = ''
    export PROMETHEUS_PORT=${toString prometheusPort}
  '';
}
