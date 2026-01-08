{ pkgs, lib, config, ... }:

let
  cfg = config.services.elasticmq;
  types = lib.types;
in
{
  options.services.elasticmq = {
    enable = lib.mkEnableOption "elasticmq-server";

    package = lib.mkOption {
      type = types.package;
      description = "Which package of elasticmq-server-bin to use";
      default = pkgs.elasticmq-server-bin;
      defaultText = lib.literalExpression "pkgs.elasticmq-server-bin";
    };

    readinessHost = lib.mkOption {
      type = types.str;
      description = "Host address for the SQS-REST readiness check";
      default = "127.0.0.1";
    };

    readinessPort = lib.mkOption {
      type = lib.types.int;
      description = "Port for the SQS-REST readiness check";
      default = 9324;
    };

    settings = lib.mkOption {
      type = types.lines;
      default = "";
      description = "Configuration for elasticmq-server";
    };
  };

  config = lib.mkIf cfg.enable {
    processes.elasticmq-server = {
      exec = ''
        JAVA_TOOL_OPTIONS=\"-Dconfig.file=${pkgs.writeText "elasticmq-server.conf" cfg.settings}\" ${cfg.package}/bin/elasticmq-server
      '';

      process-compose = {
        readiness_probe = {
          initial_delay_seconds = 4;
          http_get = {
            host = cfg.readinessHost;
            path = "/health";
            port = cfg.readinessPort;
          };
        };
      };
    };
  };
}
