{ pkgs, ... }:
let
  port = 7700;
in
{

  config = {
    services.meilisearch = {
      enable = true;
      listenPort = port;
      noAnalytics = true;
      listenAddress = "127.0.0.1";
    };

    scripts.meilisearch-healthcheck.exec = ''
      RUNNING=$(${pkgs.curl}/bin/curl -s 127.0.0.1:${toString port}/health | grep "available")

      if [[ -z "$RUNNING" ]]; then
        exit 1
      else
        exit 0
      fi
    '';

    enterTest = ''
      wait_for_port ${toString port}

      timeout 5 bash -c "until meilisearch-healthcheck; do sleep 1; done"
    '';
  };
}
