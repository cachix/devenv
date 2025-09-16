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
      "disable-dotenv" = true;
      "port" = if !cfg.unixSocket.enable then toString cfg.port else null;
      # Prevent the TUI from immediately closing if all processes fail.
      # Improves the UX by letting users inspect the logs.
      "keep-project" = cfg.tui.enable;
      "unix-socket" =
        if cfg.unixSocket.enable
        then cfg.unixSocket.path
        else null;
      # TODO: move -t (for tui) here. We need a newer nixpkgs for optionValueSeparator = "=".
    };

    process.manager.command = lib.mkDefault ''
      # Ensure the log directory exists
      mkdir -p "${config.env.DEVENV_STATE}/process-compose"

      ${if cfg.unixSocket.enable then ''
      # Check if process-compose server is already running on the socket
      if [ -S "${cfg.unixSocket.path}" ]; then
        echo "Attaching to existing process-compose server at ${cfg.unixSocket.path}"
        exec ${cfg.package}/bin/process-compose --unix-socket "${cfg.unixSocket.path}" attach "$@"
      fi
      '' else ""}

      # Start a new process-compose server if none exists
      ${cfg.package}/bin/process-compose \
        ${lib.cli.toGNUCommandLineShell { } config.process.manager.args} \
        -t="''${PC_TUI_ENABLED:-${lib.boolToString cfg.tui.enable}}" \
        up "$@" &
    '';

    packages = [ cfg.package ];

    process.managers.process-compose = {
      configFile = lib.mkDefault (settingsFormat.generate "process-compose.yaml" cfg.settings);
      settings = {
        version = lib.mkDefault "0.5";
        is_strict = lib.mkDefault true;
        log_location = lib.mkDefault "${config.env.DEVENV_STATE}/process-compose/process-compose.log";
        processes = lib.mapAttrs
          (name: value:
            let
              command =
                if value.process-compose.is_elevated or false
                then "${config.task.package}/bin/devenv-tasks run --mode all devenv:processes:${name}"
                else "exec ${config.task.package}/bin/devenv-tasks run --mode all devenv:processes:${name}";
            in
            { inherit command; } // value.process-compose
          )
          config.processes;
      };
    };
  };
}
