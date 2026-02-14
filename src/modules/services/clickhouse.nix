{ pkgs, lib, config, ... }:

let
  cfg = config.services.clickhouse;
  types = lib.types;

  # Port allocation
  basePort = cfg.port;
  baseHttpPort = cfg.httpPort;
  allocatedPort = config.processes.clickhouse-server.ports.main.value;
  allocatedHttpPort = config.processes.clickhouse-server.ports.http.value;
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
      type = types.port;
      description = "Which port to run clickhouse on.";
      default = 9000;
    };

    httpPort = lib.mkOption {
      type = types.port;
      description = "Which http port to run clickhouse on.";
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
      tcp_port: ${toString allocatedPort}
      http_port: ${toString allocatedHttpPort}
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
      ports.main.allocate = basePort;
      ports.http.allocate = baseHttpPort;
      exec = "exec clickhouse-server --config-file=${pkgs.writeText "clickhouse-config.yaml" cfg.config}";

      ready = {
        exec = "${cfg.package}/bin/clickhouse-client --port ${toString allocatedPort} -q 'SELECT 1'";
        initial_delay = 2;
        timeout = 4;
        failure_threshold = 5;
      };
    };
  };
}
