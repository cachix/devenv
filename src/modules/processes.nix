{ config, options, lib, pkgs, ... }:
let
  types = lib.types;

  # Get primops from _module.args (set via specialArgs in bootstrapLib.nix)
  # Use default empty attrset if not available (e.g., when evaluated without devenv CLI)
  devenvPrimops = config._module.args.devenvPrimops or { };

  # Capture primop for use in submodule (specialArgs don't propagate to types.submodule)
  # Signature: allocatePort processName portName basePort
  allocatePort = devenvPrimops.allocatePort or (_proc: _port: base: base);

  # Port type factory - needs process name for stable cache key
  mkPortType = processName: types.submodule ({ config, name, ... }: {
    options = {
      allocate = lib.mkOption {
        type = types.port;
        description = "Base port for auto-allocation (increments until free)";
        example = 8080;
      };

      value = lib.mkOption {
        type = types.port;
        readOnly = true;
        description = "Resolved port value (allocated by devenv)";
        defaultText = lib.literalMD "Allocated port based on `allocate`";
        # Pass process name and port name for stable caching across evaluations
        default = allocatePort processName name config.allocate;
      };
    };
  });

  processType = types.submodule ({ config, name, ... }: {
    options = {
      exec = lib.mkOption {
        type = types.str;
        description = "Bash code to run the process.";
      };

      ports = lib.mkOption {
        type = types.attrsOf (mkPortType name);
        default = { };
        description = ''
          Named ports with auto-allocation for this process.

          Define ports with a base value and devenv will automatically find
          a free port starting from that base, incrementing until available.

          The resolved port is available via `config.processes.<name>.ports.<port>.value`.
        '';
        example = lib.literalExpression ''
          {
            http.allocate = 8080;
            admin.allocate = 9000;
          }
        '';
      };

      cwd = lib.mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Working directory to run the process in. If not specified, the current working directory will be used.";
      };

      process-compose = lib.mkOption {
        # TODO: type up as a submodule for discoverability
        type = (pkgs.formats.yaml { }).type;
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

    process.taskCommandsBase = lib.mkOption {
      type = types.attrsOf types.str;
      internal = true;
      description = "The base command to run each process through devenv-tasks, supporting before/after task dependencies.";
    };

    process.taskCommands = lib.mkOption {
      type = types.attrsOf types.str;
      internal = true;
      description = "The command to run each process through devenv-tasks with exec prefix for proper signal handling.";
    };
  };

  config = lib.mkMerge [
    # Always resolve and enable the correct process manager implementation.
    # Making this conditional on processes has the potential of triggering infinite recursion.
    #
    # Suppose the user defines processes with a `mkIf` conditional on an env var.
    # The module system needs to merge:
    #
    #   config.processes -> config.env -> any mkIf'd env in the process manager ->  config.process.managers.${implementation}.enable -> config.processes -> ...
    #
    # Infinite recursion, oh my!
    {
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
    }

    (lib.mkIf options.processes.isDefined {
      # Create tasks for each defined process
      tasks = lib.mapAttrs'
        (name: process: {
          name = "devenv:processes:${name}";
          value = {
            type = "process";
            exec = process.exec;
            cwd = process.cwd;
            # Always show output for process tasks so process-compose can capture it
            showOutput = true;
          };
        })
        config.processes;

      # Provide the devenv-tasks command for each process so process managers can use it
      # to support before/after task dependencies
      process.taskCommandsBase = lib.mapAttrs
        (name: _: "${config.task.package}/bin/devenv-tasks run --task-file ${config.task.config} --mode all devenv:processes:${name}")
        config.processes;

      # With exec prefix for proper signal handling (derived from base)
      process.taskCommands = lib.mapAttrs
        (name: cmd: "exec ${cmd}")
        config.process.taskCommandsBase;

      procfile =
        pkgs.writeText "procfile" (lib.concatStringsSep "\n"
          (lib.mapAttrsToList (name: _: "${name}: ${config.process.taskCommands.${name}}")
            config.processes));

      procfileEnv =
        let
          envList =
            lib.mapAttrsToList
              (name: value: "${name}=${builtins.toJSON value}")
              config.env;
        in
        pkgs.writeText "procfile-env" (lib.concatStringsSep "\n" envList);

      procfileScript = pkgs.writeShellScript "devenv-up" ''
        ${lib.optionalString config.devenv.debug "set -x"}

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
    })
  ];
}
