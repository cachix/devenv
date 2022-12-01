{ pkgs, lib, config, ... }:

let
  cfg = config.postgres;
  types = lib.types;
  createDatabase = lib.optionalString cfg.createDatabase ''
    echo "CREATE DATABASE ''${USER:-$(id -nu)};" | postgres --single -E postgres
  '';
  setupScript = pkgs.writeShellScriptBin "setup-postgres" ''
    set -euo pipefail
    export PATH=${cfg.package}/bin:${pkgs.coreutils}/bin

    # Setup config
    cat >> "$PGDATA/postgresql.conf" <<EOF
      listen_addresses = '${cfg.listen_addresses}'
      port = ${toString cfg.port}
      unix_socket_directories = '$PGDATA'
    EOF

    # Abort if the data dir already exists
    [[ ! -d "$PGDATA" ]] || exit 0
    initdb ${lib.concatStringsSep " " cfg.initdbArgs}
    ${createDatabase}
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
    };

    listen_addresses = lib.mkOption {
      type = types.str;
      description = "Listen address";
      default = "";
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
      default = [ "--no-locale" ];
      example = [ "--data-checksums" "--allow-group-access" ];
      description = ''
        Additional arguments passed to `initdb` during data dir
        initialisation.
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
