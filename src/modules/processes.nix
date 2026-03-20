{ config, options, lib, pkgs, ... }:
let
  types = lib.types;
  listenType = import ./lib/listen.nix { inherit lib; };
  readyType = import ./lib/ready.nix { inherit lib; };

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

  parseProcessDep = import ./lib/parse-process-dep.nix { inherit lib; };

  processType = types.submodule ({ config, name, ... }: {
    options = {
      start = lib.mkOption {
        type = types.submodule {
          options = {
            enable = lib.mkOption {
              type = types.bool;
              default = true;
              description = ''
                Whether to start this process automatically with `devenv up`.

                Disabled processes are still visible in the TUI as stopped
                and can be started manually by selecting them and pressing Enter.
              '';
            };
          };
        };
        default = { };
        description = "Auto-start configuration for this process.";
      };

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

          Requires devenv 2.0+.
        '';
        example = lib.literalExpression ''
          {
            http.allocate = 8080;
            admin.allocate = 9000;
          }
        '';
      };

      env = lib.mkOption {
        type = types.attrsOf types.str;
        default = { };
        description = "Environment variables to set for this process.";
      };

      cwd = lib.mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Working directory to run the process in. If not specified, the current working directory will be used.";
      };

      ready = lib.mkOption {
        type = types.nullOr readyType;
        default = null;
        description = ''
          How to determine if this process is ready to serve.

          Requires devenv 2.0+.
        '';
      };

      restart = lib.mkOption {
        type = types.submodule {
          options = {
            on = lib.mkOption {
              type = types.enum [ "never" "always" "on_failure" ];
              default = "on_failure";
              description = "When to restart: never, always, or on_failure.";
            };
            max = lib.mkOption {
              type = types.nullOr types.int;
              default = 5;
              description = "Maximum restart attempts. null = unlimited.";
            };
            window = lib.mkOption {
              type = types.nullOr types.ints.unsigned;
              default = null;
              description = "Sliding window in seconds for restart rate limiting. null = lifetime limit.";
            };
          };
        };
        default = { };
        description = "Process restart policy.";
      };

      listen = lib.mkOption {
        type = types.listOf listenType;
        default = [ ];
        description = ''
          Socket activation configuration for systemd-style socket passing.

          Requires devenv 2.0+.
        '';
        example = [
          {
            name = "http";
            kind = "tcp";
            address = "127.0.0.1:8080";
          }
          {
            name = "admin";
            kind = "unix_stream";
            path = "$DEVENV_STATE/admin.sock";
            mode = 384; # 0o600
          }
        ];
      };

      watchdog = lib.mkOption {
        type = types.nullOr (types.submodule {
          options = {
            usec = lib.mkOption {
              type = types.int;
              description = "Watchdog interval in microseconds";
            };

            require_ready = lib.mkOption {
              type = types.bool;
              default = true;
              description = "Require READY=1 notification before enforcing watchdog";
            };
          };
        });
        default = null;
        description = ''
          Systemd watchdog configuration.

          Requires devenv 2.0+.
        '';
        example = lib.literalExpression ''
          {
            usec = 30000000; # 30 seconds
            require_ready = true;
          }
        '';
      };

      after = lib.mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = ''
          Tasks that must be ready before this process starts.
          Use task names like "devenv:processes:postgres" or "myapp:setup".
          Supports @started, @ready (default for processes), and @completed suffixes for process dependencies.
          Supports @started, @succeeded (default for tasks), and @completed suffixes for task dependencies.
        '';
        example = [ "devenv:processes:postgres" "myapp:migrations@succeeded" ];
      };

      before = lib.mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = ''
          Tasks that should start after this process is ready.
        '';
        example = [ "devenv:processes:nginx" ];
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

      linux = lib.mkOption {
        type = types.submodule {
          options = {
            capabilities = lib.mkOption {
              type = types.listOf types.str;
              default = [ ];
              description = ''
                Linux capabilities to add as ambient capabilities for this process
                (e.g., "cap_net_admin", "cap_sys_admin").

                Requires devenv 2.0+.
              '';
              example = [ "cap_net_admin" "cap_sys_admin" ];
            };
          };
        };
        default = { };
        description = ''
          Linux-specific process configuration.

          Requires devenv 2.0+.
        '';
      };

      watch = lib.mkOption {
        type = types.submodule {
          options = {
            paths = lib.mkOption {
              type = types.listOf types.path;
              default = [ ];
              description = ''
                Paths to watch for changes (files or directories).
                When files in these paths change, the process will be restarted.

                Requires devenv 2.0+.
              '';
              example = lib.literalExpression ''
                [ ./src ./config.yaml ]
              '';
            };

            extensions = lib.mkOption {
              type = types.listOf types.str;
              default = [ ];
              description = ''
                File extensions to watch (e.g., "rs", "js", "py").
                If empty, all file extensions are watched.

                Requires devenv 2.0+.
              '';
              example = [ "rs" "toml" ];
            };

            ignore = lib.mkOption {
              type = types.listOf types.str;
              default = [ ];
              description = ''
                Glob patterns to ignore (e.g., ".git", "target", "*.log").

                Requires devenv 2.0+.
              '';
              example = [ "*.log" ".git" "target" ];
            };
          };
        };
        default = { };
        description = ''
          File watching configuration for automatic process restarts.

          Requires devenv 2.0+.
        '';
        example = lib.literalExpression ''
          {
            paths = [ ./src ];
            extensions = [ "rs" "toml" ];
            ignore = [ "target" ];
          }
        '';
      };

    };

    config = lib.mkIf (implementation == "process-compose") {
      process-compose.depends_on =
        let
          deps = lib.filter (x: x != null) (map parseProcessDep config.after);
        in
        lib.listToAttrs (map
          (dep: {
            name = dep.name;
            value.condition = dep.pcCondition;
          })
          deps);
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
        default =
          if config.devenv.cli.version != null && lib.versionAtLeast config.devenv.cli.version "2.0"
          then "native"
          else "process-compose";
        defaultText = lib.literalMD "`native` for devenv 2.0+, `process-compose` otherwise";
        example = "process-compose";
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

    process.nativeConfigJson = lib.mkOption {
      type = types.package;
      internal = true;
      default = pkgs.writeText "process-config.json" (builtins.toJSON { });
      description = "JSON configuration for native process manager";
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

    (lib.mkIf options.processes.isDefined (
      let
        enabledProcesses = lib.filterAttrs (_: p: p.start.enable) config.processes;
      in
      {
        # Create tasks for all processes (native manager uses enable flag to decide auto-start)
        tasks = lib.mapAttrs'
          (name: process: {
            name = "devenv:processes:${name}";
            value = {
              type = "process";
              exec = process.exec;
              env = process.env;
              cwd = process.cwd;
              after = process.after;
              before = process.before;
              showOutput = true;
              process = {
                start.enable = process.start.enable;
                ready = process.ready;
                restart = process.restart;
                listen = process.listen;
                ports = lib.mapAttrs (_: portCfg: portCfg.value) process.ports;
                watch = process.watch;
                watchdog = process.watchdog;
              };
            };
          })
          config.processes;

        # Provide the devenv-tasks command for each enabled process so non-native process managers
        # (process-compose, mprocs) can use it to run before/after task dependencies.
        # Not used by the native manager (devenv 2.0+) which handles process tasks directly.
        process.taskCommandsBase =
          let
            ignoreProcessDepsFlag = lib.optionalString (implementation != "native") " --ignore-process-deps";
          in
          lib.mapAttrs
            (name: _: "${config.task.package}/bin/devenv-tasks run --task-file ${config.task.config} --mode all --cache-dir ${lib.escapeShellArg config.devenv.dotfile} --runtime-dir ${lib.escapeShellArg config.devenv.runtime}${ignoreProcessDepsFlag} devenv:processes:${name}")
            enabledProcesses;

        # With exec prefix for proper signal handling (derived from base)
        process.taskCommands = lib.mapAttrs
          (name: cmd: "exec ${cmd}")
          config.process.taskCommandsBase;

        procfile =
          pkgs.writeText "procfile" (lib.concatStringsSep "\n"
            (lib.mapAttrsToList (name: _: "${name}: ${config.process.taskCommands.${name}}")
              enabledProcesses));

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

        infoSections."processes" = lib.mapAttrsToList (name: process: "${name}: exec ${pkgs.writeShellScript name process.exec}") enabledProcesses;
      }
    ))
  ];
}
