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

    exec ${cfg.package}/bin/mysqld ${mysqldOptions}
  '';
  configureScript = pkgs.writeShellScriptBin "configure-mysql" ''
    set -euo pipefail

    while ! ${cfg.package}/bin/mysqladmin ping -u root --silent; do
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
          ) | ${cfg.package}/bin/mysql ${mysqlOptions} -u root -N
      fi
    '') cfg.initialDatabases}

    ${concatMapStrings (user:
      ''
        ( echo "CREATE USER IF NOT EXISTS '${user.name}'@'localhost' ${optionalString (user.password != null) "IDENTIFIED BY '${user.password}'"};"
          ${concatStringsSep "\n" (mapAttrsToList (database: permission: ''
            echo "GRANT ${permission} ON ${database} TO '${user.name}'@'localhost';"
          '') user.ensurePermissions)}
        ) | ${cfg.package}/bin/mysql ${mysqlOptions} -u root -N
    '') cfg.ensureUsers}

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

    ensureUsers = lib.mkOption {
      type = types.listOf (types.submodule {
        options = {
          name = lib.mkOption {
            type = types.str;
            description = lib.mdDoc ''
              Name of the user to ensure.
            '';
          };

          password = lib.mkOption {
            type = types.nullOr types.str;
            default = null;
            description = lib.mdDoc ''
              Password of the user to ensure.
            '';
          };

          ensurePermissions = lib.mkOption {
            type = types.attrsOf types.str;
            default = { };
            description = lib.mdDoc ''
              Permissions to ensure for the user, specified as attribute set.
              The attribute names specify the database and tables to grant the permissions for,
              separated by a dot. You may use wildcards here.
              The attribute values specfiy the permissions to grant.
              You may specify one or multiple comma-separated SQL privileges here.
              For more information on how to specify the target
              and on which privileges exist, see the
              [GRANT syntax](https://mariadb.com/kb/en/library/grant/).
              The attributes are used as `GRANT ''${attrName} ON ''${attrValue}`.
            '';
            example = literalExpression ''
              {
                "database.*" = "ALL PRIVILEGES";
                "*.*" = "SELECT, LOCK TABLES";
              }
            '';
          };
        };
      });
      default = [ ];
      description = lib.mdDoc ''
        Ensures that the specified users exist and have at least the ensured permissions.
        The MySQL users will be identified using Unix socket authentication. This authenticates the Unix user with the
        same name only, and that without the need for a password.
        This option will never delete existing users or remove permissions, especially not when the value of this
        option is changed. This means that users created and permissions assigned once through this option or
        otherwise have to be removed manually.
      '';
      example = literalExpression ''
        [
          {
            name = "devenv";
            ensurePermissions = {
              "devenv.*" = "ALL PRIVILEGES";
            };
          }
        ]
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];

    env = {
      MYSQL_HOME = config.env.DEVENV_STATE + "/mysql";
      MYSQL_UNIX_PORT = config.env.DEVENV_STATE + "/mysql.sock";
    } // (optionalAttrs (hasAttrByPath [ "mysqld" "port" ] cfg.settings) {
      MYSQL_TCP_PORT = (toString cfg.settings.mysqld.port);
    });

    scripts.mysql.exec = ''
      exec ${cfg.package}/bin/mysql ${mysqlOptions} "$@"
    '';

    scripts.mysqladmin.exec = ''
      exec ${cfg.package}/bin/mysqladmin ${mysqlOptions} "$@"
    '';

    scripts.mysqldump.exec = ''
      exec ${cfg.package}/bin/mysqldump ${mysqlOptions} "$@"
    '';

    processes.mysql.exec = "${startScript}/bin/start-mysql";
    processes.mysql-configure.exec = "${configureScript}/bin/configure-mysql";
  };
}
