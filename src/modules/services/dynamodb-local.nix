{ pkgs, lib, config, ... }:

let
  cfg = config.services.dynamodb-local;
  types = lib.types;

  # Port allocation
  portBase = cfg.port;
  allocatedPort = config.processes.dynamodb.ports.main.value;

  baseDir = config.env.DEVENV_STATE + "/dynamodb-local";
  startScript = pkgs.writeShellScript "start-dynamodb-local" ''
    set -euo pipefail

    if [[ ! -d "${baseDir}" ]]; then
      mkdir -p "${baseDir}"
    fi

    cd "${baseDir}"

    extraFlags=""
    if [[ "${toString cfg.sharedDb}" ]]; then
      extraFlags+="-sharedDb"
    fi

    exec ${config.services.dynamodb-local.package}/bin/dynamodb-local -port ${toString allocatedPort} -dbPath ${baseDir} -disableTelemetry $extraFlags
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
      description = "Listen port for DynamoDB Local.";
      default = 8000;
    };
    sharedDb = lib.mkOption {
      type = types.bool;
      default = false;
      description = ''
        If true, enables the -sharedDb flag for DynamoDB Local.
        When enabled, DynamoDB Local creates a single database file named shared-local-instance.db.
        Every program that connects to DynamoDB accesses this file. If you delete the file, you lose any data stored in it.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    processes.dynamodb = {
      ports.main.allocate = portBase;
      exec = "${startScript}";
      process-compose = {
        readiness_probe = {
          exec.command = ''
            AWS_ACCESS_KEY_ID=dummy AWS_SECRET_ACCESS_KEY=dummy AWS_DEFAULT_REGION=us-east-1 \
            ${pkgs.awscli2}/bin/aws dynamodb list-tables --endpoint-url http://127.0.0.1:${toString allocatedPort} --output text --no-cli-pager >/dev/null 2>&1
          '';
          initial_delay_seconds = 2;
          period_seconds = 10;
          timeout_seconds = 5;
          success_threshold = 1;
          failure_threshold = 5;
        };

        availability.restart = "on_failure";
      };
    };
  };
}
