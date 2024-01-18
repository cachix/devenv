{ pkgs, lib, config, ... }:

let
  cfg = config.services.dynamodb-local;
  types = lib.types;
  baseDir = config.env.DEVENV_STATE + "/dynamodb-local";
  startScript = pkgs.writeShellScript "start-dynamodb-local" ''
    set -euo pipefail

    if [[ ! -d "${baseDir}" ]]; then
      mkdir -p "${baseDir}"
    fi

    cd "${baseDir}"

    ${config.services.dynamodb-local.package}/bin/dynamodb-local -port ${toString cfg.port} -dbPath ${baseDir} -disableTelemetry
  '';
in
{
  options.services.dynamodb-local = {
    enable = lib.mkEnableOption "DynamoDB Local";

    package = lib.mkOption {
      type = types.package;
      description = "Which package of DynamoDB to use.";
      default = pkgs.dynamodb-local;
      defaultText = lib.literalExpression "pkgs.dynamodb-local";
    };

    port = lib.mkOption {
      type = types.port;
      description = "Listen address for the Dynamodb-local.";
      default = 8000;
    };
  };

  config = lib.mkIf cfg.enable {
    processes.dynamodb = {
      exec = "${startScript}";
      process-compose = {
        readiness_probe = {
          exec.command = "${pkgs.curl}/bin/curl -f -k http://127.0.0.1:${toString cfg.port}";
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
