{ pkgs
, lib
, config
, ...
}:
let
  cfg = config.services.couchdb;

  settingsFormat = pkgs.formats.ini { };
  configFile = settingsFormat.generate "couchdb.ini" cfg.settings;

  baseDir = config.env.DEVENV_STATE + "/couchdb";
  startScript = pkgs.writeShellScriptBin "start-couchdb" ''
    set -euo pipefail
    mkdir -p '${baseDir}'
    touch '${baseDir}/couchdb.uri'
    touch '${baseDir}/couchdb.ini'

    if [[ ! -e '${baseDir}/.erlang.cookie' ]]; then
      touch '${baseDir}/.erlang.cookie'
      chmod 600 '${baseDir}/.erlang.cookie'
      dd if=/dev/random bs=16 count=1 status=none | base64 > ${baseDir}/.erlang.cookie
    fi

    exec ${cfg.package}/bin/couchdb
  '';
in
{
  options.services.couchdb = {
    enable = lib.mkEnableOption "CouchDB process";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which version of CouchDB to use";
      default = pkgs.couchdb3;
      defaultText = lib.literalExpression "pkgs.couchdb3";
    };

    settings = lib.mkOption {
      type = lib.types.submodule {
        freeformType = settingsFormat.type;
        options.couchdb.database_dir = lib.mkOption {
          type = lib.types.path;
          default = baseDir;
          description = ''
            Specifies location of CouchDB database files (*.couch named). This
            location should be writable and readable for the user the CouchDB
            service runs as (couchdb by default).
          '';
        };
        options.couchdb.single_node = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = ''
            When this configuration setting is set to true, automatically create
            the system databases on startup. Must be set false for a clustered
            CouchDB installation.
          '';
        };
        options.couchdb.view_index_dir = lib.mkOption {
          type = lib.types.path;
          default = baseDir;
          description = ''
            Specifies location of CouchDB view index files. This location should
            be writable and readable for the user that runs the CouchDB service
            (couchdb by default).
          '';
        };
        options.couchdb.uri_file = lib.mkOption {
          type = lib.types.path;
          default = "${baseDir}/couchdb.uri";
          description = ''
            This file contains the full URI that can be used to access this
            instance of CouchDB. It is used to help discover the port CouchDB is
            running on (if it was set to 0 (e.g. automatically assigned any free
            one). This file should be writable and readable for the user that
            runs the CouchDB service (couchdb by default).
          '';
        };

        options.chttpd.bind_address = lib.mkOption {
          type = lib.types.str;
          default = "127.0.0.1";
          description = lib.mdDoc ''
            Defines the IP address by which CouchDB will be accessible.
          '';
        };

        options.chttpd.port = lib.mkOption {
          type = lib.types.port;
          default = 5984;
          description = lib.mdDoc ''
            Defined the port number to listen.
          '';
        };
      };
      description = ''
        CouchDB configuration.
        to know more about all settings, look at:
        <link
          xlink:href="https://docs.couchdb.org/en/stable/config/couchdb.html"
        />
      '';

      example = lib.literalExpression ''
        {
          couchdb = {
            database_dir = baseDir;
            single_node = true;
            view_index_dir = baseDir;
            uri_file = "${baseDir}/couchdb.uri";
          };
          admins = {
            "admin_username" = "pass";
          };
          chttpd = {
            bind_address = "127.0.0.1";
            port = 5984;
          };
        }
      '';
      default = { };
    };
  };
  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];
    services.couchdb.settings = {
      couchdb = {
        database_dir = baseDir;
        single_node = true;
        view_index_dir = baseDir;
        uri_file = "${baseDir}/couchdb.uri";
      };
      admins = {
        admin = "admin";
      };
      chttpd = {
        bind_address = "127.0.0.1";
        port = 5984;
      };
    };
    env.ERL_FLAGS = "-couch_ini ${cfg.package}/etc/default.ini ${configFile} '${baseDir}/couchdb.ini'";
    processes.couchdb.exec = "${startScript}/bin/start-couchdb";
  };
}
