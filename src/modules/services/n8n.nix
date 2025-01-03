{ config
, lib
, pkgs
, ...
}:

with lib;

let
  inherit (lib) types;
  cfg = config.services.n8n;
  format = pkgs.formats.json { };
  configFile = format.generate "n8n.json" cfg.settings;
in
{
  options = {
    services.n8n = {
      enable = mkEnableOption "n8n";

      address = lib.mkOption {
        type = types.str;
        description = "Listen address";
        default = "127.0.0.1";
        example = "127.0.0.1";
      };

      port = lib.mkOption {
        type = types.port;
        default = 5432;
        description = ''
          The TCP port to accept connections.
        '';
      };

      webhookUrl = lib.mkOption {
        type = lib.types.str;
        default = "";
        description = ''
          WEBHOOK_URL for n8n, in case we're running behind a reverse proxy.
          This cannot be set through configuration and must reside in an environment variable.
        '';
      };

      settings = lib.mkOption {
        type = format.type;
        default = { };
        description = ''
          Configuration for n8n, see <https://docs.n8n.io/hosting/environment-variables/configuration-methods/>
          for supported values.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [{
      assertion = cfg.enable;
      message = ''
        To use n8n, you have to enable it. (services.n8n.enable = true;)
      '';
    }];
    env = {
      N8N_PORT = cfg.port;
      N8N_LISTEN_ADDRESS = cfg.address;
      WEBHOOK_URL = "${cfg.webhookUrl}";
      N8N_CONFIG_FILES = "${configFile}";
    };

    processes.n8n = {
      exec = "${pkgs.n8n}/bin/n8n";

      process-compose = {
        readiness_probe = {
          exec.command = "${pkgs.curl}/bin/curl -f -k ${cfg.address}:${toString cfg.port}";
          initial_delay_seconds = 1;
          period_seconds = 10;
          timeout_seconds = 2;
          success_threshold = 1;
          failure_threshold = 5;
        };

        availability.restart = "on_failure";
      };
    };
  };
}
