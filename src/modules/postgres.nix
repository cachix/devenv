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
    # Abort if the data dir already exists
    [[ ! -d "$PGDATA" ]] || exit 0
    initdb ${lib.concatStringsSep " " cfg.initdbArgs}
    cat >> "$PGDATA/postgresql.conf" <<EOF
      listen_addresses = '''
      unix_socket_directories = '$PGDATA'
    EOF
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

    scripts."psql-devenv".exec = "${cfg.package}/bin/psql -h $PGDATA $@";

    processes.postgres.exec = "${startScript}/bin/start-postgres";
  };
}
