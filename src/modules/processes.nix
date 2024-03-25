{ config, lib, pkgs, ... }:
let
  types = lib.types;
  processType = types.submodule ({ config, ... }: {
    options = {
      exec = lib.mkOption {
        type = types.str;
        description = "Bash code to run the process.";
      };

      # TODO: Deprecate this option in favor of `process-managers.process-compose.settings.processes.${name}`.
      process-compose = lib.mkOption {
        type = types.attrs; # TODO: type this explicitly?
        default = { };
        description = ''
          process-compose.yaml specific process attributes.

          Example: https://github.com/F1bonacc1/process-compose/blob/main/process-compose.yaml`

          Only used when using ``process.implementation = "process-compose";``
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
  });

  implementation = config.process.implementation;
  envList =
    lib.mapAttrsToList
      (name: value: "${name}=${builtins.toJSON value}")
      config.env;
in
{
  options = {
    processes = lib.mkOption {
      type = types.attrsOf processType;
      default = { };
      description =
        "Processes can be started with ``devenv up`` and run in foreground mode.";
    };

    # TODO: Rename this from `process.${option}` to `process-manager.${option}` or `devenv.up.${option}`.
    process = {
      implementation = lib.mkOption {
        type = types.enum [ "honcho" "overmind" "process-compose" "hivemind" ];
        description = "The implementation used when performing ``devenv up``.";
        default = "process-compose";
        example = "overmind";
      };

      process-compose = lib.mkOption {
        # NOTE: https://github.com/F1bonacc1/process-compose/blob/1c706e7c300df2455de7a9b259dd35dea845dcf3/src/app/config.go#L11-L16
        type = types.attrs;
        description = ''
          Top-level process-compose.yaml options when that implementation is used.
        '';
        default = {
          version = "0.5";
          port = 9999;
          tui = true;
        };
        example = {
          version = "0.5";
          log_location = "/path/to/combined/output/logfile.log";
          log_level = "fatal";
        };
      };

      before = lib.mkOption {
        type = types.lines;
        description = "Bash code to execute before starting processes.";
        default = "";
      };

      after = lib.mkOption {
        type = types.lines;
        description = "Bash code to execute after stopping processes.";
        default = "";
      };
    };

    # INTERNAL
    # TODO: Rename these to `process-manager.${option}`
    processManagerCommand = lib.mkOption {
      type = types.str;
      internal = true;
      description = ''
        The command to run the process-manager. This is meant to be set by the process-manager.''${implementation}.
      '';
    };

    procfile = lib.mkOption {
      type = types.package;
      internal = true;
    };

    procfileEnv = lib.mkOption {
      internal = true;
      type = types.package;
    };

    procfileScript = lib.mkOption {
      type = types.package;
      internal = true;
      default = pkgs.writeShellScript "no-processes" "";
    };
  };

  config = lib.mkIf (config.processes != { }) {
    process-managers.${implementation}.enable = lib.mkDefault true;

    procfile =
      pkgs.writeText "procfile" (lib.concatStringsSep "\n"
        (lib.mapAttrsToList (name: process: "${name}: exec ${pkgs.writeShellScript name process.exec}")
          config.processes));

    procfileEnv =
      pkgs.writeText "procfile-env" (lib.concatStringsSep "\n" envList);

    procfileScript = pkgs.writeShellScript "devenv-up" ''
      ${config.process.before}

      ${config.processManagerCommand}

      backgroundPID=$!

      stop_up() {
        echo "Stopping processes..."
        kill -TERM $backgroundPID
        wait $backgroundPID
        ${config.process.after}
        echo "Processes stopped."
      }

      trap stop_up SIGINT SIGTERM

      wait
    '';

    ci = [ config.procfileScript ];

    infoSections."processes" = lib.mapAttrsToList (name: process: "${name}: exec ${pkgs.writeShellScript name process.exec}") config.processes;
  };
}
