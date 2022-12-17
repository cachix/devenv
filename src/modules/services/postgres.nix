{ pkgs, lib, config, ... }:

let
  cfg = config.services.postgres;
  types = lib.types;
  setupInitialDatabases =
    if cfg.initialDatabases != [ ] then
      (lib.concatMapStrings
        (database: ''
          echo "Checking presence of database: ${database.name}"
          # Create initial databases
          dbAlreadyExists="$(
            echo "SELECT 1 as exists FROM pg_database WHERE datname = '${database.name}';" | \
            postgres --single -E postgres | \
            ${pkgs.gnugrep}/bin/grep -c 'exists = "1"' || true
          )"
          echo $dbAlreadyExists
          if [ 1 -ne "$dbAlreadyExists" ]; then
            echo "Creating database: ${database.name}"
            echo 'create database "${database.name}";' | postgres --single -E postgres


            ${lib.optionalString (database.schema != null) ''
            echo "Applying database schema on ${database.name}"
            if [ -f "${database.schema}" ]
            then
              echo "Running file ${database.schema}"
              cat "${database.schema}" | postgres --single -E ${database.name}
            elif [ -d "${database.schema}" ]
            then
              echo "Running sql files in ${database.schema}"
              cat "${database.schema}/*.sql" | postgres --single -E ${database.name}
            else
              echo "ERROR: Could not determine how to apply schema with ${database.schema}"
              exit 1
            fi
            ''}
          fi
        '')
        cfg.initialDatabases)
    else
      lib.optionalString cfg.createDatabase ''
        echo "CREATE DATABASE ''${USER:-$(id -nu)};" | postgres --single -E postgres '';

  toStr = value:
    if true == value then
      "yes"
    else if false == value then
      "no"
    else if lib.isString value then
      "'${lib.replaceStrings [ "'" ] [ "''" ] value}'"
    else
      toString value;

  configFile = pkgs.writeText "postgresql.conf" (lib.concatStringsSep "\n"
    (lib.mapAttrsToList (n: v: "${n} = ${toStr v}") cfg.settings));
  setupScript = pkgs.writeShellScriptBin "setup-postgres" ''
    set -euo pipefail
    export PATH=${cfg.package}/bin:${pkgs.coreutils}/bin

    if [[ ! -d "$PGDATA" ]]; then
      initdb ${lib.concatStringsSep " " cfg.initdbArgs}
      ${setupInitialDatabases}
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
  imports = [
    (lib.mkRenamedOptionModule [ "postgres" "enable" ] [
      "services"
      "postgres"
      "enable"
    ])
  ];

  options.services.postgres = {
    enable = lib.mkEnableOption ''
      Add postgreSQL process script.
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
        Create a database named like current user on startup. Only applies when initialDatabases is an empty list.
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

    initialDatabases = lib.mkOption {
      type = types.listOf (types.submodule {
        options = {
          name = lib.mkOption {
            type = types.str;
            description = ''
              The name of the database to create.
            '';
          };
          schema = lib.mkOption {
            type = types.nullOr types.path;
            default = null;
            description = ''
              The initial schema of the database; if null (the default),
              an empty database is created.
            '';
          };
        };
      });
      default = [ ];
      description = ''
        List of database names and their initial schemas that should be used to create databases on the first startup
        of Postgres. The schema attribute is optional: If not specified, an empty database is created.
      '';
      example = [
        {
          name = "foodatabase";
          schema = lib.literalExpression "./foodatabase.sql";
        }
        { name = "bardatabase"; }
      ];
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package startScript ];

    env.PGDATA = config.env.DEVENV_STATE + "/postgres";
    env.PGHOST = config.env.PGDATA;
    env.PGPORT = cfg.port;

    services.postgres.settings = {
      listen_addresses = cfg.listen_addresses;
      port = cfg.port;
      unix_socket_directories = config.env.PGDATA;
    };

    processes.postgres = {
      exec = "${startScript}/bin/start-postgres";

      process-compose = {
        readiness_probe = {
          exec.command = "${cfg.package}/bin/pg_isready -h $PGDATA template1";
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
