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
        type = lib.types.str;
        default = "${config.devenv.runtime}/pc.sock";
        defaultText = lib.literalExpression "\${config.devenv.runtime}/pc.sock";
        description = "Override the path to the unix socket.";
      };
    };

    tui = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable the TUI (Terminal User Interface)";
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
    env = {
      PC_CONFIG_FILES = toString cfg.configFile;
      PC_SOCKET_PATH = if cfg.unixSocket.enable then cfg.unixSocket.path else null;
    };

    process.manager.args = {
      "config" = cfg.configFile;
      "port" = if !cfg.unixSocket.enable then toString cfg.port else null;
      "unix-socket" =
        if cfg.unixSocket.enable
        then cfg.unixSocket.path
        else null;
      # TODO: move -t (for tui) here. We need a newer nixpkgs for optionValueSeparator = "=".
    };

    process.manager.command = lib.mkDefault ''
      ${cfg.package}/bin/process-compose \
        ${lib.cli.toGNUCommandLineShell { } config.process.manager.args} \
        -t="''${PC_TUI_ENABLED:-${lib.boolToString cfg.tui.enable}}" \
        up "$@" &
    '';

    packages = [ cfg.package ] ++ lib.optional cfg.tui.enable pkgs.ncurses;

    process.managers.process-compose = {
      configFile = lib.mkDefault (settingsFormat.generate "process-compose.yaml" cfg.settings);
      settings = {
        version = lib.mkDefault "0.5";
        is_strict = lib.mkDefault true;
        # Filter out the recursive PC_CONFIG_FILES env.
        # Otherwise, we would get a loop:
        #   PC_CONFIG_FILES -> configFile -> settings -> PC_CONFIG_FILES -> ...
        environment = lib.mapAttrsToList
          (name: value: "${name}=${toString value}")
          (builtins.removeAttrs config.env [ "PC_CONFIG_FILES" ]);
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
