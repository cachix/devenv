{ pkgs, config, lib, ... }:
let
  cfg = config.process.managers.process-compose;
  settingsFormat = pkgs.formats.yaml { };
in
{
  options.process.managers.process-compose = {
    enable = lib.mkEnableOption "process-compose as the process manager" // {
      internal = true;
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.process-compose;
      defaultText = lib.literalExpression "pkgs.process-compose";
      description = "The process-compose package to use.";
    };

    port = lib.mkOption {
      type = lib.types.int;
      default = 8080;
      description = ''
        The port to bind the process-compose server to.

        Not used when `unixSocket.enable` is true.
      '';
    };

    unixSocket = {
      enable = lib.mkEnableOption "running the process-compose server over unix domain sockets instead of tcp" // {
        default = true;
      };

      path = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = "${config.devenv.runtime}/pc.sock";
        defaultText = lib.literalExpression "\${config.devenv.runtime}/pc.sock";
        description = "Override the path to the unix socket.";
      };
    };

    tui = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable the TUI";
      };
    };

    configFile = lib.mkOption {
      type = lib.types.path;
      internal = true;
    };

    settings = lib.mkOption {
      type = settingsFormat.type;
      description = ''
        Top-level process-compose.yaml options

        Example: https://github.com/F1bonacc1/process-compose/blob/main/process-compose.yaml`
      '';
      default = { };
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
    process.manager.args = {
      "config" = cfg.configFile;
      # -U enables automatic UDS mode, creating the socket in $TMP.
      "U" = cfg.unixSocket.enable && cfg.unixSocket.path == null;
      "unix-socket" = cfg.unixSocket.path;
      "tui" = "\${PC_TUI_ENABLED:-${lib.boolToString cfg.tui.enable}}";
    };

    process.manager.command = lib.mkDefault ''
      ${cfg.package}/bin/process-compose \
        ${lib.concatStringsSep " " (lib.cli.toGNUCommandLine {} config.process.manager.args)} \
        up "$@" &
    '';

    packages = [ cfg.package ];

    process.managers.process-compose = {
      configFile = lib.mkDefault (settingsFormat.generate "process-compose.yaml" cfg.settings);
      settings = {
        version = "0.5";
        is_strict = true;
        environment = lib.mapAttrsToList
          (name: value: "${name}=${toString value}")
          config.env;
        processes = lib.mapAttrs
          (name: value:
            let
              scriptPath = pkgs.writeShellScript name value.exec;
              command =
                if value.process-compose.is_elevated or false
                then "${scriptPath}"
                else "exec ${scriptPath}";
            in
            { inherit command; } // value.process-compose
          )
          config.processes;
      };
    };
  };
}
