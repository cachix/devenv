{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.surrealdb;

  basePort = cfg.port;
  allocatedPort = config.processes.surrealdb.ports.main.value;

  stateDir = "${config.env.DEVENV_STATE}/surrealdb";
in
{
  options.services.surrealdb = {
    enable = lib.mkEnableOption "Surrealdb process";

    package = lib.mkPackageOption pkgs "surrealdb" { };

    port = lib.mkOption {
      type = lib.types.port;
      default = 8080;
      description = "The TCP port to accept connection.";
    };

    username = lib.mkOption {
      type = with lib.types; nullOr str;
      default = null;
      description = "Sets master username for the database";
    };

    password = lib.mkOption {
      type = with lib.types; nullOr str;
      default = null;
      description = "Sets master password for the database";
    };

    schema = lib.mkOption {
      type = with lib.types; nullOr (either path str);
      default = null;
      description = "Path to a SurrealQL (.surql) file that will be imported when starting the server";
    };

    namespace = lib.mkOption {
      type = with lib.types; nullOr str;
      default = "main";
      description = "Sets the defauilt namespace";
    };

    database = lib.mkOption {
      type = with lib.types; nullOr str;
      default = "main";
      description = "Sets the default database";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];

    env = {
      SURREAL_USER = cfg.username;
      SURREAL_PASS = cfg.password;
      SURREAL_PATH = stateDir;
      SURREAL_DATABASE = cfg.database;
      SURREAL_NAMESPACE = cfg.namespace;
      SURREAL_ENDPOINT = "ws://localhost:${toString allocatedPort}";
    };

    processes.surrealdb = {
      ports.main.allocate = basePort;
      exec =
        let
          args =
            let
              mk = key: value: lib.optionalString (value != null) "--${key} ${value}";
            in
            builtins.concatStringsSep " " [
              "--no-banner"
              "--bind 0.0.0.0:${toString allocatedPort}"
              (mk "username" cfg.username)
              (mk "password" cfg.password)
              (mk "import-file" cfg.schema)
            ];

          startScript =
            let
              surreal = "${cfg.package}/bin/surreal";
            in
            pkgs.writeShellScriptBin "start-surreal" ''
              set -euo pipefail

              if [[ ! -d "$SURREAL_PATH" ]]; then
                mkdir -p "$SURREAL_PATH"
              fi

              ${surreal} start ${args} memory
            '';
        in
        "${startScript}/bin/start-surreal";
    };
  };
}
