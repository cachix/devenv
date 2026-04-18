{ pkgs, lib, config, ... }:

let
  cfg = config.services.clickhouse;
  types = lib.types;

  # Port allocation
  basePort = cfg.port;
  baseHttpPort = cfg.httpPort;
  allocatedPort = config.processes.clickhouse-server.ports.main.value;
  allocatedHttpPort = config.processes.clickhouse-server.ports.http.value;
  format = pkgs.formats.yaml { };
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

    usersConfig = lib.mkOption {
      type = format.type;
      default = { };
      example = lib.literalExpression ''
        {
          profiles = {};

          users = {
            default = {
              profile = "default";
              password_sha256_hex = "36dd292533174299fb0c34665df468bb881756ca9eaf9757d0cfde38f9ededa1";  # `echo -n verysecret | sha256sum`
            };
          };
        }
      '';
      description = ''
        Your `users.yaml` as a Nix attribute set.
        Check the [documentation](https://clickhouse.com/docs/operations/configuration-files#user-settings)
        for possible options.
      '';
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
      mysql_port: 0
      postgresql_port: 0
      default_profile: default
      default_database: default
      path: ${config.env.DEVENV_STATE}/clickhouse
      tmp_path: ${config.env.DEVENV_STATE}/clickhouse/tmp
      user_files_path: ${config.env.DEVENV_STATE}/clickhouse/user_files
      format_schema_path: ${config.env.DEVENV_STATE}/clickhouse/format_schemas
      access_control_path: ${config.env.DEVENV_STATE}/clickhouse/access
      custom_cached_disks_base_directory: ${config.env.DEVENV_STATE}/clickhouse/caches
      user_directories:
        users_xml:
          path: ${config.env.DEVENV_STATE}/clickhouse/server/users.xml
        local_directory:
          path: ${config.env.DEVENV_STATE}/clickhouse/access

      macros:
        shard: 1
        replica: localhost

      remote_servers:
        default:
          shard:
            replica:
              host: localhost
              port: ${toString allocatedPort}

      keeper_server:
        tcp_port: 9181
        server_id: 1
        log_storage_path: ${config.env.DEVENV_STATE}/clickhouse/coordination/log
        snapshot_storage_path: ${config.env.DEVENV_STATE}/clickhouse/coordination/snapshots
        raft_configuration:
          server:
            id: 1
            hostname: localhost
            port: 9234
    '';

    processes.clickhouse-server = {
      ports.main.allocate = basePort;
      ports.http.allocate = baseHttpPort;
      exec = ''
        mkdir -p ${config.env.DEVENV_STATE}/clickhouse/{server/config.d,server/users.d,tmp,user_files,format_schemas,access,caches,coordination/snapshots,coordination/log}
        install -m 644 ${cfg.package}/etc/clickhouse-server/users.xml ${config.env.DEVENV_STATE}/clickhouse/server/users.xml
        install -m 644 ${cfg.package}/etc/clickhouse-server/config.xml ${config.env.DEVENV_STATE}/clickhouse/server/config.xml
        install -m 644 ${pkgs.writeText "clickhouse-config.yaml" cfg.config} ${config.env.DEVENV_STATE}/clickhouse/server/config.d/clickhouse-config.yaml
        install -m 644 ${format.generate "users.yaml" cfg.usersConfig} ${config.env.DEVENV_STATE}/clickhouse/server/users.d/users.yaml
        exec clickhouse-server --config-file=${config.env.DEVENV_STATE}/clickhouse/server/config.xml
      '';

      ready = {
        exec = "${cfg.package}/bin/clickhouse-client --port ${toString allocatedPort} -q 'SELECT 1'";
        initial_delay = 2;
        probe_timeout = 4;
        failure_threshold = 5;
      };
    };
  };
}
