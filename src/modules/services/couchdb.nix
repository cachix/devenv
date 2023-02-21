{ pkgs
, lib
, config
, ...
}:
let
  cfg = config.services.couchdb;
  types = lib.types;
  baseDir = config.env.DEVENV_STATE + "/couchdb";
  settingsFormat = pkgs.formats.ini { };
  configFile = settingsFormat.generate "couchdb.ini" cfg.settings;

  startScript = pkgs.writeShellScriptBin "start-couchdb" ''
    set -euo pipefail
    if [[ ! -d "${baseDir}" ]]; then
      mkdir -p "${baseDir}"
      touch ${baseDir}/couchdb.uri
    fi

    if ! test -e ${baseDir}/.erlang.cookie; then
      touch ${baseDir}/.erlang.cookie
      chmod 600 ${baseDir}/.erlang.cookie
      dd if=/dev/random bs=16 count=1 | base64 > ${baseDir}/.erlang.cookie
    fi

    exec ${cfg.package}/bin/couchdb
  '';
in
{
  options.services.couchdb = {
    enable = lib.mkEnableOption "CouchDB process";

    package = lib.mkOption {
      type = types.package;
      description = "Which version of CouchDB to use";
      default = pkgs.couchdb3;
      defaultText = lib.literalExpression "pkgs.couchdb3";
    };

    settings = lib.mkOption {
      type = settingsFormat.type;
      description = ''
        CouchDB configuration.
        to know more about all settings, look at:
        https://docs.couchdb.org/en/stable/config/couchdb.html
      '';
      default = {
        couchdb = {
          database_dir = baseDir;
          single_node = true;
          viewIndexDir = baseDir;
          uriFile = "${baseDir}/couchdb.uri";
        };
        admins = {
          admin = "admin";
        };
        chttpd = {
          bindAddress = "127.0.0.1";
          port = 5984;
          logFile = "${baseDir}/couchdb.log";
        };
      };
      example = lib.literalExpression ''
        {
          couchdb = {
            database_dir = baseDir;
            single_node = true;
            viewIndexDir = baseDir;
            uriFile = "${baseDir}/couchdb.uri";
          };
          admins = {
            "admin_username" = "pass";
          };
          chttpd = {
            bindAddress = "127.0.0.1";
            port = 5984;
            logFile = "${baseDir}/couchdb.log";
          };
        }
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];
    env.ERL_FLAGS = "-couch_ini ${configFile}";
    processes.couchdb.exec = "${startScript}/bin/start-couchdb";
  };
}
