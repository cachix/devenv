{ pkgs, lib, config, ... }:

with lib;

let
  cfg = config.services.mongodb;

  setupScript = pkgs.writeShellScriptBin "setup-mongodb" ''
    set -euo pipefail
    # Abort if the data dir already exists
    [[ ! -d "$MONGODBDATA" ]] || exit 0
    mkdir -p "$MONGODBDATA"
  '';

  startScript = pkgs.writeShellScriptBin "start-mongodb" ''
    set -euo pipefail
    ${setupScript}/bin/setup-mongodb
    exec ${cfg.package}/bin/mongod ${
      lib.concatStringsSep " " cfg.additionalArgs
    } -dbpath "$MONGODBDATA"
  '';

  configureScript = pkgs.writeShellScriptBin "configure-mongodb" ''
    set -euo pipefail

    mongodArgs=(${lib.concatStringsSep " " cfg.additionalArgs})
    mongoShellArgs=""

    # Loop over the arguments, check if it contains --port
    # If it does grab the port which must be the following arg
    # wanted to keep the additionalArgs to not break any existing
    # configs using it.
    for i in "''${!mongodArgs[@]}"; do
       if [[ "''${mongodArgs[$i]}" = "--port" ]]; then
           mongoShellArgs="--port ''${mongodArgs[i + 1]}"
           break
       fi
    done

    while ! ${pkgs.mongosh}/bin/mongosh --quiet --eval "{ ping: 1 }" ''${mongoShellArgs} 2>&1 >/dev/null ; do
        sleep 1
    done

    if [ "${cfg.initDatabaseUsername}" ] && [ "${cfg.initDatabasePassword}" ]; then
        echo "Creating initial user"
        rootAuthDatabase="admin"
        ${pkgs.mongosh}/bin/mongosh ''${mongoShellArgs} "$rootAuthDatabase" >/dev/null <<-EOJS
            db.createUser({
                user: "${cfg.initDatabaseUsername}",
                pwd: "${cfg.initDatabasePassword}",``
                roles: [ { role: 'root', db: "$rootAuthDatabase" } ]
            })
    EOJS
    fi

    # We need to keep this process running otherwise all processes stop
    tail -f /dev/null
  '';

in
{
  imports = [
    (lib.mkRenamedOptionModule [ "mongodb" "enable" ] [
      "services"
      "mongodb"
      "enable"
    ])
  ];

  options.services.mongodb = {
    enable = mkEnableOption "MongoDB process and expose utilities";

    package = mkOption {
      type = types.package;
      description = "Which MongoDB package to use.";
      default = pkgs.mongodb;
      defaultText = lib.literalExpression "pkgs.mongodb";
    };

    additionalArgs = lib.mkOption {
      type = types.listOf types.lines;
      default = [ "--noauth" ];
      example = [ "--port" "27017" "--noauth" ];
      description = ''
        Additional arguments passed to `mongod`.
      '';
    };

    initDatabaseUsername = lib.mkOption {
      type = types.str;
      default = "";
      example = "mongoadmin";
      description = ''
        This used in conjunction with initDatabasePassword, create a new user and set that user's password. This user is created in the admin authentication database and given the role of root, which is a "superuser" role.
      '';
    };

    initDatabasePassword = lib.mkOption {
      type = types.str;
      default = "";
      example = "secret";
      description = ''
        This used in conjunction with initDatabaseUsername, create a new user and set that user's password. This user is created in the admin authentication database and given the role of root, which is a "superuser" role.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package pkgs.mongodb-tools pkgs.mongosh ];

    env.MONGODBDATA = config.env.DEVENV_STATE + "/mongodb";

    processes.mongodb.exec = "${startScript}/bin/start-mongodb";
    processes.mongodb-configure.exec =
      "${configureScript}/bin/configure-mongodb";
  };
}
