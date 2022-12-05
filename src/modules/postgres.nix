{ pkgs, lib, config, ... }:

let
  cfg = config.postgres;
  types = lib.types;
  createDatabase = lib.optionalString cfg.createDatabase ''
    echo "CREATE DATABASE ''${USER:-$(id -nu)};" | postgres --single -E postgres
  '';

  toStr = value:
    if true == value then "yes"
    else if false == value then "no"
    else if lib.isString value then "'${lib.replaceStrings ["'"] ["''"] value}'"
    else toString value;

  configFile = pkgs.writeText "postgresql.conf" (lib.concatStringsSep "\n" (lib.mapAttrsToList (n: v: "${n} = ${toStr v}") cfg.settings));
  setupScript = pkgs.writeShellScriptBin "setup-postgres" ''
    set -euo pipefail
    export PATH=${cfg.package}/bin:${pkgs.coreutils}/bin

    if [[ ! -d "$PGDATA" ]]; then
      initdb ${lib.concatStringsSep " " cfg.initdbArgs}
      ${createDatabase}
    fi

    # Setup config
    cp ${configFile} "$PGDATA/postgresql.conf"
  '';
  startScript = pkgs.writeShellScriptBin "start-postgres" ''
    set -euo pipefail
    ${setupScript}/bin/setup-postgres
    exec ${cfg.package}/bin/postgres
  '';
in
{
  options.postgres = {
    enable = lib.mkEnableOption ''
      Add postgreSQL process and psql-devenv script.
    '';

    package = lib.mkOption {
      type = types.package;
      description = "Which version of postgres to use";
      default = pkgs.postgresql;
      defaultText = "pkgs.postgresql";
      example = lib.literalExpression ''
        # see https://github.com/NixOS/nixpkgs/blob/master/pkgs/servers/sql/postgresql/packages.nix for full list
        pkgs.postgresql_13.withPackages (p: [ p.pg_cron p.timescaledb p.pg_partman ]);
      '';
    };

    listen_addresses = lib.mkOption {
      type = types.str;
      description = "Listen address";
      default = "";
      example = "127.0.0.1";
    };

    port = lib.mkOption {
      type = types.port;
      default = 5432;
      description = ''
        The TCP port to accept connections.
      '';
    };

    createDatabase = lib.mkOption {
      type = types.bool;
      default = true;
      description = ''
        Create a database named like current user on startup.
      '';
    };

    initdbArgs = lib.mkOption {
      type = types.listOf types.lines;
      default = [ "--locale=C" "--encoding=UTF8" ];
      example = [ "--data-checksums" "--allow-group-access" ];
      description = ''
        Additional arguments passed to `initdb` during data dir
        initialisation.
      '';
    };

    settings = lib.mkOption {
      type = with types; attrsOf (oneOf [ bool float int str ]);
      default = { };
      description = lib.mdDoc ''
        PostgreSQL configuration. Refer to
        <https://www.postgresql.org/docs/11/config-setting.html#CONFIG-SETTING-CONFIGURATION-FILE>
        for an overview of `postgresql.conf`.
        ::: {.note}
        String values will automatically be enclosed in single quotes. Single quotes will be
        escaped with two single quotes as described by the upstream documentation linked above.
        :::
      '';
      example = lib.literalExpression ''
        {
          log_connections = true;
          log_statement = "all";
          logging_collector = true
          log_disconnections = true
          log_destination = lib.mkForce "syslog";
        }
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];

    env.PGDATA = config.env.DEVENV_STATE + "/postgres";
    env.PGHOST = config.env.PGDATA;
    env.PGPORT = cfg.port;

    postgres.settings = {
      listen_addresses = cfg.listen_addresses;
      port = cfg.port;
      unix_socket_directories = config.env.PGDATA;
    };

    processes.postgres = {
      exec = "${startScript}/bin/start-postgres";

      process-compose = {
        readiness_probe = {
          exec.command = "${cfg.package}/bin/pg_isready -h $PGDATA";
          initial_delay_seconds = 2;
          period_seconds = 10;
          timeout_seconds = 4;
          success_threshold = 1;
          failure_threshold = 5;
        };
      };
    };
  };
}
