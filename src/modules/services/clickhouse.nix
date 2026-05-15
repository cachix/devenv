{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.services.clickhouse;
  types = lib.types;

  # Port allocation
  basePort = cfg.port;
  baseHttpPort = cfg.httpPort;
  baseKeeperPort = cfg.keeperPort;
  baseRaftPort = cfg.raftPort;
  allocatedPort = config.processes.clickhouse-server.ports.main.value;
  allocatedHttpPort = config.processes.clickhouse-server.ports.http.value;
  allocatedKeeperPort = config.processes.clickhouse-server.ports.keeper.value;
  allocatedRaftPort = config.processes.clickhouse-server.ports.raft.value;

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

    keeperPort = lib.mkOption {
      type = types.port;
      description = "Which port to run clickhouse keeper service on.";
      default = 9181;
    };

    raftPort = lib.mkOption {
      type = types.port;
      description = "Which http port to use clickhouse keeper for raft consensus.";
      default = 9234;
    };

    timezone = lib.mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "Which timezone to use for ClickHouse.";
    };

    macros.enable = lib.mkOption {
      type = types.bool;
      default = false;
      description = "Whether to enable macros in ClickHouse.";
    };

    remoteServers.enable = lib.mkOption {
      type = types.bool;
      default = false;
      description = "Whether to enable remote_servers in ClickHouse.";
    };

    keeper.enable = lib.mkOption {
      type = types.bool;
      default = false;
      description = "Whether to enable keeper_server in ClickHouse.";
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
      ${lib.optionalString (cfg.timezone != null) "timezone: ${cfg.timezone}"}
      tcp_port: ${toString allocatedPort}
      http_port: ${toString allocatedHttpPort}
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

      ${lib.optionalString cfg.macros.enable ''
        macros:
          shard: 1
          replica: localhost
      ''}

      ${lib.optionalString cfg.remoteServers.enable ''
        remote_servers:
          default:
            shard:
              replica:
                host: localhost
                port: ${toString allocatedPort}
      ''}

      ${lib.optionalString cfg.keeper.enable ''
        keeper_server:
          tcp_port: ${toString allocatedKeeperPort}
          server_id: 1
          log_storage_path: ${config.env.DEVENV_STATE}/clickhouse/coordination/log
          snapshot_storage_path: ${config.env.DEVENV_STATE}/clickhouse/coordination/snapshots
          raft_configuration:
            server:
              id: 1
              hostname: localhost
              port: ${toString allocatedRaftPort}
      ''}
    '';

    tasks."devenv:clickhouse:setup" = {
      description = "Setup ClickHouse";
      exec = ''
        mkdir -p ${config.env.DEVENV_STATE}/clickhouse/{server/config.d,server/users.d,tmp,user_files,format_schemas,access,caches,coordination/snapshots,coordination/log}
        install -m 644 ${cfg.package}/etc/clickhouse-server/users.xml ${config.env.DEVENV_STATE}/clickhouse/server/users.xml
        install -m 644 ${cfg.package}/etc/clickhouse-server/config.xml ${config.env.DEVENV_STATE}/clickhouse/server/config.xml
        install -m 644 ${pkgs.writeText "clickhouse-config.yaml" cfg.config} ${config.env.DEVENV_STATE}/clickhouse/server/config.d/clickhouse-config.yaml
        install -m 644 ${format.generate "users.yaml" cfg.usersConfig} ${config.env.DEVENV_STATE}/clickhouse/server/users.d/users.yaml
      '';
      before = [ "devenv:processes:clickhouse-server" ];
    };

    processes.clickhouse-server = {
      ports.main.allocate = basePort;
      ports.http.allocate = baseHttpPort;
      ports.keeper.allocate = baseKeeperPort;
      ports.raft.allocate = baseRaftPort;
      exec = "exec clickhouse-server --config-file=${config.env.DEVENV_STATE}/clickhouse/server/config.xml";

      ready = {
        exec = "${cfg.package}/bin/clickhouse-client --port ${toString allocatedPort} --user default ${
          lib.optionalString (
            config.services.clickhouse.usersConfig.users.default ? password
          ) "--password ${config.services.clickhouse.usersConfig.users.default.password}"
        } -q 'SELECT 1'";
        initial_delay = 2;
        probe_timeout = 4;
        failure_threshold = 5;
      };
    };
  };
}
