{ config, options, lib, pkgs, ... }:
let
  types = lib.types;

  processType = types.submodule ({ config, ... }: {
    options = {
      exec = lib.mkOption {
        type = types.str;
        description = "Bash code to run the process.";
      };

      process-compose = lib.mkOption {
        type = types.attrs; # TODO: type this explicitly?
        default = { };
        description = ''
          process-compose.yaml specific process attributes.

          Example: https://github.com/F1bonacc1/process-compose/blob/main/process-compose.yaml`

          Only used when using ``process.manager.implementation = "process-compose";``
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

  supportedImplementations = builtins.attrNames options.process.managers;

  implementation = config.process.manager.implementation;
  envList =
    lib.mapAttrsToList
      (name: value: "${name}=${builtins.toJSON value}")
      config.env;
in
{
  imports =
    (map (name: lib.mkRenamedOptionModule [ "process" name ] [ "process" "manager" name ]) [ "after" "before" "implementation" ])
    ++ [
      (lib.mkRenamedOptionModule [ "process" "process-compose" "port" ] [ "process" "managers" "process-compose" "port" ])
      (lib.mkRenamedOptionModule [ "process" "process-compose" "tui" ] [ "process" "managers" "process-compose" "tui" "enable" ])
      (lib.mkRenamedOptionModule [ "process" "process-compose" "unix-socket" ] [ "process" "managers" "process-compose" "unixSocket" "path" ])
      (lib.mkRenamedOptionModule [ "processManagerCommand" ] [ "process" "manager" "command" ])
      (lib.mkRenamedOptionModule [ "process-managers" ] [ "process" "managers" ])
    ];

  options = {
    processes = lib.mkOption {
      type = types.attrsOf processType;
      default = { };
      description =
        "Processes can be started with ``devenv up`` and run in the foreground.";
    };

    process.manager = {
      implementation = lib.mkOption {
        type = types.enum supportedImplementations;
        description = "The process manager to use when running processes with ``devenv up``.";
        default = "process-compose";
        example = "overmind";
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

      command = lib.mkOption {
        type = types.str;
        internal = true;
        description = ''
          The command to run the process manager.

          This is meant to be set by the process.manager.''${implementation}.
          If overriding this, ``process.manager.args`` will not be applied.
        '';
      };

      args = lib.mkOption {
        type = types.attrs;
        description = ''
          Additional arguments to pass to the process manager.
        '';
      };
    };

    # INTERNAL

    procfile = lib.mkOption {
      type = types.package;
      internal = true;
    };

    procfileEnv = lib.mkOption {
      type = types.package;
      internal = true;
    };

    procfileScript = lib.mkOption {
      type = types.package;
      internal = true;
      default = pkgs.writeShellScript "no-processes" "";
    };
  };

  config = lib.mkIf (config.processes != { }) {
    assertions = [{
      assertion =
        let
          enabledImplementations =
            lib.pipe supportedImplementations [
              (map (name: config.process.managers.${name}.enable))
              (lib.filter lib.id)
            ];
        in
        lib.length enabledImplementations == 1;
      message = ''
        Only a single process manager can be enabled at a time.
      '';
    }];

    process.managers.${implementation}.enable = lib.mkDefault true;

    procfile =
      pkgs.writeText "procfile" (lib.concatStringsSep "\n"
        (lib.mapAttrsToList (name: process: "${name}: exec ${pkgs.writeShellScript name process.exec}")
          config.processes));

    procfileEnv =
      pkgs.writeText "procfile-env" (lib.concatStringsSep "\n" envList);

    procfileScript = pkgs.writeShellScript "devenv-up" ''
      ${config.process.manager.before}

      ${config.process.manager.command}

      backgroundPID=$!

      down() {
        echo "Stopping processes..."
        kill -TERM $backgroundPID
        wait $backgroundPID
        ${config.process.manager.after}
        echo "Processes stopped."
      }

      trap down SIGINT SIGTERM

      wait
    '';

    ci = [ config.procfileScript ];

    infoSections."processes" = lib.mapAttrsToList (name: process: "${name}: exec ${pkgs.writeShellScript name process.exec}") config.processes;
  };
}
