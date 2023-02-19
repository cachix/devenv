{ pkgs
, lib
, config
, ...
}:
let
  cfg = config.services.couchdb;
  types = lib.types;
  baseDir = config.env.DEVENV_STATE + "/couchdb";
  configFile = pkgs.writeText "couchdb.ini" (
    ''
      [couchdb]
      database_dir = ${baseDir}
      uri_file = ${cfg.uriFile}
      view_index_dir = ${cfg.viewIndexDir}
      single_node = true

      [admins]
      ${cfg.adminUser} = ${cfg.adminPass}

      [chttpd]
      port = ${toString cfg.port}
      bind_address = ${cfg.bindAddress}
      [log]
      file = ${cfg.logFile}
    ''
  );
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
  imports = [
    (lib.mkRenamedOptionModule [ "couchdb" "enable" ] [
      "services"
      "couchdb"
      "enable"
    ])
  ];

  options.services.couchdb = {
    enable = lib.mkEnableOption "Add CouchDB process script.";

    package = lib.mkOption {
      type = types.package;
      description = "Which version of CouchDB to use";
      default = pkgs.couchdb3;
      defaultText = lib.literalExpression "pkgs.couchdb3";
    };

    adminUser = lib.mkOption {
      type = types.str;
      default = "admin";
      description = lib.mdDoc ''
        Couchdb (i.e. fauxton) account with permission for all dbs and
        tasks.
      '';
    };

    adminPass = lib.mkOption {
      type = types.str;
      default = "admin";
      description = lib.mdDoc ''
        Couchdb (i.e. fauxton) account with permission for all dbs and
        tasks.
      '';
    };

    uriFile = lib.mkOption {
      type = types.path;
      default = "${baseDir}/couchdb.uri";
      description = lib.mdDoc ''
        This file contains the full URI that can be used to access this
        instance of CouchDB. It is used to help discover the port CouchDB is
        running on (if it was set to 0 (e.g. automatically assigned any free
        one). This file should be writable and readable for the user that
        runs the CouchDB service (couchdb by default).
      '';
    };

    viewIndexDir = lib.mkOption {
      type = types.path;
      default = baseDir;
      description = lib.mdDoc ''
        Specifies location of CouchDB view index files. This location should
        be writable and readable for the user that runs the CouchDB service
        (couchdb by default).
      '';
    };

    bindAddress = lib.mkOption {
      type = types.str;
      default = "127.0.0.1";
      description = lib.mdDoc ''
        Defines the IP address by which CouchDB will be accessible.
      '';
    };

    port = lib.mkOption {
      type = types.port;
      default = 5984;
      description = lib.mdDoc ''
        Defined the port number to listen.
      '';
    };

    logFile = lib.mkOption {
      type = types.path;
      default = "${baseDir}/couchdb.log";
      description = lib.mdDoc ''
        Specifies the location of file for logging output.
      '';
    };

    extraConfig = lib.mkOption {
      type = types.lines;
      default = "";
      description = lib.mdDoc ''
        Extra configuration. Overrides any other cofiguration.
      '';
    };

    argsFile = lib.mkOption {
      type = types.path;
      default = "${cfg.package}/etc/vm.args";
      defaultText = lib.literalExpression ''"config.${pkgs.couchdb3}/etc/vm.args"'';
      description = lib.mdDoc ''
        vm.args configuration. Overrides Couchdb's Erlang VM parameters file.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package startScript ];
    env.ERL_FLAGS = "-couch_ini ${configFile} ${pkgs.writeText
    "couchdb-extra.ini" cfg.extraConfig}";
    processes.couchdb.exec = "${startScript}/bin/start-couchdb";
  };
}
