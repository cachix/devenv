{ pkgs, lib, config, ... }:

with lib;

let
  cfg = config.mysql;
  isMariaDB = getName cfg.package == getName pkgs.mariadb;
  format = pkgs.formats.ini { listsAsDuplicateKeys = true; };
  configFile = format.generate "my.cnf" cfg.settings;
  mysqlOptions = "--defaults-file=${configFile}";
  mysqldOptions = "${mysqlOptions} --datadir=$MYSQL_HOME --basedir=${cfg.package}";
  startScript = pkgs.writeShellScriptBin "start-mysql" ''
    set -euo pipefail

    if [[ ! -d "$MYSQL_HOME" ]]; then
      mkdir -p "$MYSQL_HOME"
      ${if isMariaDB then "${cfg.package}/bin/mysql_install_db" else "${cfg.package}/bin/mysqld"} ${mysqldOptions} ${optionalString (!isMariaDB) "--initialize-insecure"}
    fi

    exec ${cfg.package}/bin/mysqld ${mysqldOptions} --socket=$MYSQL_UNIX_PORT
  '';
  configureScript = pkgs.writeShellScriptBin "configure-mysql" ''
    set -euo pipefail

    while ! ${cfg.package}/bin/mysqladmin ping -u root --socket=$MYSQL_UNIX_PORT --silent; do
      sleep 1
    done

    ${concatMapStrings (database: ''
      # Create initial databases
      if ! test -e "$MYSQL_HOME/${database.name}"; then
          echo "Creating initial database: ${database.name}"
          ( echo 'create database `${database.name}`;'
            ${optionalString (database.schema != null) ''
            echo 'use `${database.name}`;'
            # TODO: this silently falls through if database.schema does not exist,
            # we should catch this somehow and exit, but can't do it here because we're in a subshell.
            if [ -f "${database.schema}" ]
            then
                cat ${database.schema}
            elif [ -d "${database.schema}" ]
            then
                cat ${database.schema}/mysql-databases/*.sql
            fi
            ''}
          ) | ${cfg.package}/bin/mysql --socket=$MYSQL_UNIX_PORT -u root -N
      fi
    '') cfg.initialDatabases}

    # We need to sleep until infinity otherwise all processes stop
    sleep infinity
  '';
in
{
  options.mysql = {
    enable = mkEnableOption "Add mysql process and expose utilities.";

    package = mkOption {
      type = types.package;
      description = "Which package of mysql to use";
      default = pkgs.mysql80;
      defaultText = "pkgs.mysql80";
    };

    settings = mkOption {
      type = format.type;
      default = { };
      description = ''
        MySQL configuration
      '';
      example = literalExpression ''
        {
          mysqld = {
            key_buffer_size = "6G";
            table_cache = 1600;
            log-error = "/var/log/mysql_err.log";
            plugin-load-add = [ "server_audit" "ed25519=auth_ed25519" ];
          };
          mysqldump = {
            quick = true;
            max_allowed_packet = "16M";
          };
        }
      '';
    };

    initialDatabases = mkOption {
      type = types.listOf (types.submodule {
        options = {
          name = mkOption {
            type = types.str;
            description = ''
              The name of the database to create.
            '';
          };
          schema = mkOption {
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
        of MySQL. The schema attribute is optional: If not specified, an empty database is created.
      '';
      example = [
        { name = "foodatabase"; schema = literalExpression "./foodatabase.sql"; }
        { name = "bardatabase"; }
      ];
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];

    env.MYSQL_HOME = config.env.DEVENV_STATE + "/mysql";
    env.MYSQL_UNIX_PORT = config.env.DEVENV_STATE + "/mysql.sock";

    scripts.mysql.exec = ''
      exec ${cfg.package}/bin/mysql ${mysqlOptions} --socket=$MYSQL_UNIX_PORT $@
    '';

    scripts.mysqladmin.exec = ''
      exec ${cfg.package}/bin/mysqladmin ${mysqlOptions} --socket=$MYSQL_UNIX_PORT $@
    '';

    scripts.mysqldump.exec = ''
      exec ${cfg.package}/bin/mysqldump ${mysqlOptions} --socket=$MYSQL_UNIX_PORT $@
    '';

    processes.mysql.exec = "${startScript}/bin/start-mysql";
    processes.mysql-configure.exec = "${configureScript}/bin/configure-mysql";
  };
}
