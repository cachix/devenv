{ pkgs, ... }:
{
  services.prometheus = {
    enable = true;
    port = 9090;
    storage.path = "/tmp/prometheus-1";
    scrapeConfigs = [
      {
        job_name = "prometheus";
        static_configs = [{
          targets = [ "localhost:9090" ];
        }];
      }
    ];
    globalConfig = {
      scrape_interval = "1s"; # Short interval for quick testing
      evaluation_interval = "1s";
    };
  };

  scripts.ping-prometheus.exec = ''
    ${pkgs.curl}/bin/curl -sf http://localhost:9090/-/healthy
  '';
}
