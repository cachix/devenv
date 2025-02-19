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
      RUNNING=$(${pkgs.curl}/bin/curl 127.0.0.1:${toString port}/health | grep "available")

      if [[ -z "$RUNNING" ]]; then
        exit 1
      else
        exit 0
      fi
    '';

    enterTest = ''
      set -e

      wait_for_port ${toString port}

      # Give meilisearch time to initialize
      sleep 5

      meilisearch-healthcheck
    '';
  };
}
