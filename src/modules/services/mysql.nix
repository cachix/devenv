{ pkgs
, lib
, config
, ...
}:
with lib; let
  cfg = config.services.mysql;
  isMariaDB = getName cfg.package == getName pkgs.mariadb;
  format = pkgs.formats.ini { listsAsDuplicateKeys = true; };
  configFile = format.generate "my.cnf" cfg.settings;
  # Generate an empty config file to not resolve globally installed MySQL config like in /etc/my.cnf or ~/.my.cnf
  emptyConfig = format.generate "empty.cnf" { };
  mysqlOptions =
    if !cfg.useDefaultsExtraFile then
      "--defaults-file=${configFile}"
    else
      "--defaults-extra-file=${configFile}";
  mysqldOptions = "--defaults-file=${configFile} --datadir=$MYSQL_HOME --basedir=${cfg.package}";

  mysqlWrapped = pkgs.writeShellScriptBin "mysql" ''
    exec ${cfg.package}/bin/mysql ${mysqlOptions} "$@"
  '';

  mysqlWrappedEmpty = pkgs.writeShellScriptBin "mysql" ''
    exec ${cfg.package}/bin/mysql --defaults-file=${emptyConfig} "$@"
  '';

  mysqladminWrapped = pkgs.writeShellScriptBin "mysqladmin" ''
    exec ${cfg.package}/bin/mysqladmin ${mysqlOptions} "$@"
  '';

  mysqladminWrappedEmpty = pkgs.writeShellScriptBin "mysqladmin" ''
    exec ${cfg.package}/bin/mysqladmin --defaults-file=${emptyConfig} "$@"
  '';

  mysqldumpWrapped = pkgs.writeShellScriptBin "mysqldump" ''
    exec ${cfg.package}/bin/mysqldump ${mysqlOptions} "$@"
  '';

  initDatabaseCmd =
    if isMariaDB
    then "${cfg.package}/bin/mysql_install_db ${mysqldOptions} --auth-root-authentication-method=normal"
    else "${cfg.package}/bin/mysqld ${mysqldOptions} --default-time-zone=SYSTEM --initialize-insecure";

  importTimeZones =
    if (cfg.importTimeZones != null)
    then cfg.importTimeZones
    else hasAttrByPath [ "settings" "mysqld" "default-time-zone" ] cfg;

  configureTimezones = ''
    # Start a temp database with the default-time-zone to import tz data
    # and hide the temp database from the configureScript by setting a custom socket
    nohup ${cfg.package}/bin/mysqld ${mysqldOptions} --socket="$DEVENV_RUNTIME/config.sock" --skip-networking --default-time-zone=SYSTEM &

    while ! MYSQL_PWD="" ${mysqladminWrappedEmpty}/bin/mysqladmin --socket="$DEVENV_RUNTIME/config.sock" ping -u root --silent; do
      sleep 1
    done

    ${cfg.package}/bin/mysql_tzinfo_to_sql ${pkgs.tzdata}/share/zoneinfo/ | MYSQL_PWD="" ${mysqlWrappedEmpty}/bin/mysql --socket="$DEVENV_RUNTIME/config.sock" -u root mysql

    # Shutdown the temp database
    MYSQL_PWD="" ${mysqladminWrappedEmpty}/bin/mysqladmin --socket="$DEVENV_RUNTIME/config.sock" shutdown -u root
  '';

  startScript = pkgs.writeShellScriptBin "start-mysql" ''
    set -euo pipefail

    if [[ ! -d "$MYSQL_HOME" || ! -f "$MYSQL_HOME/ibdata1" ]]; then
      mkdir -p "$MYSQL_HOME"
      ${initDatabaseCmd}
      ${optionalString importTimeZones configureTimezones}
    fi

    exec ${cfg.package}/bin/mysqld ${mysqldOptions}
  '';

  configureScript = pkgs.writeShellScriptBin "configure-mysql" ''
    PATH="${lib.makeBinPath [cfg.package pkgs.coreutils]}:$PATH"
    set -euo pipefail

    while ! MYSQL_PWD="" ${mysqladminWrappedEmpty}/bin/mysqladmin ping -u root --silent; do
      echo "Sleeping 1s while we wait for MySQL to come up"
      sleep 1
    done

    ${concatMapStrings (database: ''
        # Create initial databases
        exists="$(
          MYSQL_PWD="" ${mysqlWrappedEmpty}/bin/mysql -u root -sB information_schema \
            <<< 'select count(*) from schemata where schema_name = "${database.name}"'
        )"
        if [[ "$exists" -eq 0 ]]; then
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
          ) | MYSQL_PWD="" ${mysqlWrappedEmpty}/bin/mysql -u root -N
        else
          echo "Database ${database.name} exists, skipping creation."
        fi
      '')
      cfg.initialDatabases}

    ${concatMapStrings (user: ''
        echo "Adding user: ${user.name}"
        ${optionalString (user.password != null) "password='${user.password}'"}
        ( echo "CREATE USER IF NOT EXISTS '${user.name}'@'localhost' ${optionalString (user.password != null) "IDENTIFIED BY '$password'"};"
          ${concatStringsSep "\n" (mapAttrsToList (database: permission: ''
            echo 'GRANT ${permission} ON ${database} TO `${user.name}`@`localhost`;'
          '')
          user.ensurePermissions)}
        ) | MYSQL_PWD="" ${mysqlWrappedEmpty}/bin/mysql -u root -N
      '')
      cfg.ensureUsers}

    # We need to sleep until infinity otherwise all processes stop
    sleep infinity
  '';
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "mysql" "enable" ] [ "services" "mysql" "enable" ])
  ];

  options.services.mysql = {
    enable = mkEnableOption "MySQL process and expose utilities";

    package = mkOption {
      type = types.package;
      description = "Which package of MySQL to use";
      default = pkgs.mariadb;
      defaultText = lib.literalExpression "pkgs.mariadb";
    };

    settings = mkOption {
      type = types.lazyAttrsOf (types.lazyAttrsOf types.anything);
      default = { };
      description = ''
        MySQL configuration.
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
      example = literalExpression ''
        [
          { name = "foodatabase"; schema = ./foodatabase.sql; }
          { name = "bardatabase"; }
        ]
      '';
    };

    importTimeZones = lib.mkOption {
      type = types.nullOr types.bool;
      default = null;
      description = ''
        Whether to import tzdata on the first startup of the mysql server
      '';
    };

    useDefaultsExtraFile = lib.mkOption {
      type = types.bool;
      default = false;
      description = ''
        Whether to use defaults-exta-file for the mysql command instead of defaults-file.
        This is useful if you want to provide a config file on the command line.
        However this can problematic if you have MySQL installed globaly because its config might leak into your environment.
        This option does not affect the mysqld command.
      '';
    };

    ensureUsers = lib.mkOption {
      type = types.listOf (types.submodule {
        options = {
          name = lib.mkOption {
            type = types.str;
            description = ''
              Name of the user to ensure.
            '';
          };

          password = lib.mkOption {
            type = types.nullOr types.str;
            default = null;
            description = ''
              Password of the user to ensure.
            '';
          };

          ensurePermissions = lib.mkOption {
            type = types.attrsOf types.str;
            default = { };
            description = ''
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
      description = ''
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

    env =
      {
        MYSQL_HOME = config.env.DEVENV_STATE + "/mysql";
        MYSQL_UNIX_PORT = config.env.DEVENV_RUNTIME + "/mysql.sock";
        MYSQLX_UNIX_PORT = config.env.DEVENV_RUNTIME + "/mysqlx.sock";
      }
      // (optionalAttrs (hasAttrByPath [ "mysqld" "port" ] cfg.settings) {
        MYSQL_TCP_PORT = toString cfg.settings.mysqld.port;
      });

    scripts.mysql.exec = ''
      ${mysqlWrapped}/bin/mysql "$@"
    '';

    scripts.mysqladmin.exec = ''
      ${mysqladminWrapped}/bin/mysqladmin "$@"
    '';

    scripts.mysqldump.exec = ''
      ${mysqldumpWrapped}/bin/mysqldump "$@"
    '';

    processes.mysql.exec = "${startScript}/bin/start-mysql";
    processes.mysql-configure.exec = "${configureScript}/bin/configure-mysql";
  };
}
