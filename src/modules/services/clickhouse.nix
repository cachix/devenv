{ pkgs, lib, config, ... }:

let
  cfg = config.services.clickhouse;
  types = lib.types;
in
{
  options.services.clickhouse = {
    enable = lib.mkEnableOption "clickhouse-server";

    package = lib.mkOption {
      type = types.package;
      description = "Which package of clickhouse to use";
      default = pkgs.clickhouse;
      defaultText = lib.literalExpression "pkgs.clickhouse";
    };

    port = lib.mkOption {
      type = types.int;
      description = "Which port to run clickhouse on";
      default = 9000;
    };

    httpPort = lib.mkOption {
      type = types.int;
      description = "Which http port to run clickhouse on";
      default = 8123;
    };

    config = lib.mkOption {
      type = types.lines;
      description = "ClickHouse configuration in YAML.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];
    services.clickhouse.config = ''
      logger:
        level: warning
        console: 1
      tcp_port: ${toString cfg.port}
      http_port: ${toString cfg.httpPort}
      default_profile: default
      default_database: default
      path: ${config.env.DEVENV_STATE}/clickhouse
      tmp_path: ${config.env.DEVENV_STATE}/clickhouse/tmp
      user_files_path: ${config.env.DEVENV_STATE}/clickhouse/user_files
      format_schema_path: ${config.env.DEVENV_STATE}/clickhouse/format_schemas
      user_directories:
        users_xml:
          path: ${cfg.package}/etc//clickhouse-server/users.xml
    '';
    processes.clickhouse-server = {
      exec = "clickhouse-server --config-file=${pkgs.writeText "clickhouse-config.yaml" cfg.config}";

      process-compose = {
        readiness_probe = {
          exec.command = "${cfg.package}/bin/clickhouse-client --port ${toString cfg.port} -q 'SELECT 1'";
          initial_delay_seconds = 2;
          period_seconds = 10;
          timeout_seconds = 4;
          success_threshold = 1;
          failure_threshold = 5;
        };

        availability.restart = "on_failure";
      };
    };
  };
}
