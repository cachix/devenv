{ pkgs, lib, config, ... }:

with lib;

let
  cfg = config.mongodb;

  setupScript = pkgs.writeShellScriptBin "setup-mongodb" ''
    set -euo pipefail
    # Abort if the data dir already exists
    [[ ! -d "$MONGODBDATA" ]] || exit 0
    mkdir -p "$MONGODBDATA"
  '';

  startScript = pkgs.writeShellScriptBin "start-mongodb" ''
    set -euo pipefail
    ${setupScript}/bin/setup-mongodb
    exec ${cfg.package}/bin/mongod ${lib.concatStringsSep " " cfg.additionalArgs} -dbpath "$MONGODBDATA"
  '';
in
{
  options.mongodb = {
    enable = mkEnableOption "Add MongoDB process and expose utilities.";

    package = mkOption {
      type = types.package;
      description = "Which MongoDB package to use.";
      default = pkgs.mongodb;
      defaultText = "pkgs.mongodb";
    };

    additionalArgs = lib.mkOption {
      type = types.listOf types.lines;
      default = [ "--noauth" ];
      example = [ "--port" "27017" "--noauth" ];
      description = ''
        Additional arguments passed to `mongod`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.mongodb-tools
    ];

    env.MONGODBDATA = config.env.DEVENV_STATE + "/mongodb";

    processes.mongodb.exec = "${startScript}/bin/start-mongodb";
  };
}
