{ pkgs, config, lib, ... }:
let
  cfg = config.process-managers.process-compose;
  settingsFormat = pkgs.formats.yaml { };
in
{
  options.process-managers.process-compose = {
    enable = lib.mkEnableOption "process-compose as process-manager";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.process-compose;
      defaultText = lib.literalExpression "pkgs.process-compose";
      description = "The process-compose package to use.";
    };
    configFile = lib.mkOption {
      type = lib.types.path;
      internal = true;
    };
    settings = lib.mkOption {
      type = settingsFormat.type;
      default = { };
      description = ''
        process-compose.yaml specific process attributes.

        Example: https://github.com/F1bonacc1/process-compose/blob/main/process-compose.yaml`
      '';
      example = {
        environment = [ "ENVVAR_FOR_THIS_PROCESS_ONLY=foobar" ];
        availability = {
          restart = "on_failure";
          backoff_seconds = 2;
          max_restarts = 5; # default: 0 (unlimited)
        };
        depends_on.some-other-process.condition =
          "process_completed_successfully";
      };
    };
  };
  config = lib.mkIf cfg.enable {
    processManagerCommand = ''
      ${cfg.package}/bin/process-compose --config ${cfg.configFile} \
        --port ''${PC_HTTP_PORT:-${toString config.process.process-compose.port}} \
        --tui=''${PC_TUI_ENABLED:-${toString config.process.process-compose.tui}} \
        up "$@" &
    '';

    packages = [ cfg.package ];

    process-managers.process-compose = {
      configFile = settingsFormat.generate "process-compose.yaml" cfg.settings;
      settings = {
        version = "0.5";
        is_strict = true;
        port = lib.mkDefault 9999;
        tui = lib.mkDefault true;
        environment = lib.mapAttrsToList
          (name: value: "${name}=${toString value}")
          config.env;
        processes = lib.mapAttrs
          (name: value: { command = "exec ${pkgs.writeShellScript name value.exec}"; } // value.process-compose)
          config.processes;
      };
    };

  };
}
