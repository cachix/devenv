{ pkgs, lib, config, ... }@inputs:
let
  types = lib.types;
  listenType = import ./lib/listen.nix { inherit lib; };
  readyType = import ./lib/ready.nix { inherit lib; };

  # Attempt to evaluate devenv-tasks using the exact nixpkgs used by the root devenv flake.
  # If the locked input is not what we expect, fall back to evaluating with the user's nixpkgs.
  #
  # In theory:
  #   - The tasks binary will be built by CI and uploaded to devenv.cachix.org
  #   - Only bumps to the nixpkgs in the root devenv flake will trigger a re-eval of devenv-tasks
  devenv-tasks =
    let
      lock = builtins.fromJSON (builtins.readFile ./../../flake.lock);
      lockedNixpkgs = lock.nodes.nixpkgs.locked;
      devenvPkgs =
        if lockedNixpkgs.type == "github" then
          let
            source = pkgs.fetchFromGitHub {
              inherit (lockedNixpkgs) owner repo rev;
              hash = lock.nodes.nixpkgs.locked.narHash;
            };
          in
          import source { system = pkgs.stdenv.system; }
        else
          pkgs;
      workspace = devenvPkgs.callPackage ./../../workspace.nix { };
    in
    workspace.crates.devenv-tasks-fast-build;

  taskType = types.submodule
    ({ name, config, ... }:
      let
        mkCommand = command: isStatus:
          if builtins.isNull command
          then null
          else
            let
              binary =
                if config.binary != null
                then "${pkgs.lib.getBin config.package}/bin/${config.binary}"
                else pkgs.lib.getExe config.package;
              isBash =
                if config.binary != null
                then config.binary == "bash"
                else config.package.meta.mainProgram or null == "bash";
              # Output exports in a format the Rust executor can parse
              # Format: DEVENV_EXPORT:<base64-encoded-var>=<base64-encoded-value>
              # Base64 encoding handles special characters safely
              exportVars = vars: ''
                for _var in ${lib.concatStringsSep " " vars}; do
                  if [ -n "''${!_var+x}" ]; then
                    _var_b64=$(printf '%s' "$_var" | base64 -w0)
                    _val_b64=$(printf '%s' "''${!_var}" | base64 -w0)
                    echo "DEVENV_EXPORT:$_var_b64=$_val_b64"
                  fi
                done
              '';
            in
            pkgs.writeScript name ''
              #!${binary}
              ${lib.optionalString (!isStatus && isBash) "set -e"}
              ${command}
              ${lib.optionalString (config.exports != [] && !isStatus) (exportVars config.exports)}
            '';
      in
      {
        options = {
          type = lib.mkOption {
            type = types.enum [ "oneshot" "process" ];
            default = "oneshot";
            description = ''
              Type of task:
              - oneshot: Task runs once and completes (default)
              - process: Task is a long-running process
            '';
          };
          exec = lib.mkOption {
            type = types.nullOr types.str;
            default = null;
            description = "Command to execute the task.";
          };
          binary = lib.mkOption {
            type = types.nullOr types.str;
            description = "Override the binary name from the default `package.meta.mainProgram`.";
            default = null;
          };
          package = lib.mkOption {
            type = types.package;
            default = pkgs.bash;
            defaultText = lib.literalExpression "pkgs.bash";
            description = "Package to install for this task.";
          };
          command = lib.mkOption {
            type = types.nullOr types.package;
            internal = true;
            default = mkCommand config.exec false;
            description = "Path to the script to run.";
          };
          status = lib.mkOption {
            type = types.nullOr types.str;
            default = null;
            description = "Check if the command should be ran";
          };
          statusCommand = lib.mkOption {
            type = types.nullOr types.package;
            internal = true;
            default = mkCommand config.status true;
            description = "Path to the script to run.";
          };
          execIfModified = lib.mkOption {
            type = types.listOf types.str;
            default = [ ];
            description = "Paths to files that should trigger a task execution if modified.";
          };
          config = lib.mkOption {
            type = types.attrsOf types.anything;
            internal = true;
            default = {
              name = name;
              type = config.type;
              description = config.description;
              status = config.statusCommand;
              after = config.after;
              before = config.before;
              command = config.command;
              input = config.input;
              exec_if_modified = config.execIfModified;
              env = config.env;
              cwd = config.cwd;
              show_output = config.showOutput;
              process = {
                ready = config.ready;
                restart = config.restart;
                listen = config.listen;
                ports = config.ports;
                watch = config.watch;
                watchdog = config.watchdog;
              };
            };
            description = "Internal configuration for the task.";
          };
          env = lib.mkOption {
            type = types.attrsOf types.str;
            default = { };
            description = "Environment variables to set for this task.";
          };
          exports = lib.mkOption {
            type = types.listOf types.str;
            default = [ ];
            description = "List of environment variables to export.";
          };
          description = lib.mkOption {
            type = types.str;
            default = "";
            description = "Description of the task.";
          };
          showOutput = lib.mkOption {
            type = types.bool;
            default = false;
            description = "Always show task output (stdout and stderr), regardless of whether the task succeeds or fails.";
          };
          after = lib.mkOption {
            type = types.listOf types.str;
            description = ''
              List of tasks that must complete before this task runs.

              Here's a helpful mnemonic to remember: This task runs *after* these tasks.

              You can append a suffix to control dependency behavior:
              - `task@started` - wait for task to begin execution
              - `task` or `task@ready` - wait for task to be ready/healthy (default for processes, processes only)
              - `task@succeeded` - wait for task to exit successfully (default for tasks, tasks only)
              - `task@completed` - wait for task to finish, regardless of exit code (soft dependency)

              Example: `after = [ "pnpm:install@completed" ];` allows this task to run
              even if pnpm:install fails.
            '';
            default = [ ];
          };
          before = lib.mkOption {
            type = types.listOf types.str;
            description = ''
              List of tasks that depend on this task completing first.

              Here's a helpful mnemonic to remember: This task runs *before* these tasks.

              You can append a suffix to control dependency behavior:
              - `task@started` - the dependent waits for this task to begin execution
              - `task` or `task@ready` - the dependent waits for this task to be ready/healthy (default for processes, processes only)
              - `task@succeeded` - the dependent waits for this task to exit successfully (default for tasks, tasks only)
              - `task@completed` - the dependent waits for this task to finish (soft dependency)
            '';
            default = [ ];
          };
          input = lib.mkOption {
            type = types.attrsOf types.anything;
            default = { };
            description = "Input values for the task, encoded as JSON.";
          };
          cwd = lib.mkOption {
            type = types.nullOr types.str;
            default = null;
            description = "Working directory to run the task in. If not specified, the current working directory will be used.";
          };

          ready = lib.mkOption {
            type = types.nullOr readyType;
            default = null;
            description = "How to determine if this process task is ready to serve.";
          };

          # Process-specific configuration (only used when type = "process")
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
            description = "Process restart policy. Only used when type = \"process\".";
          };

          ports = lib.mkOption {
            type = types.attrsOf types.port;
            default = { };
            description = ''
              Allocated ports for this process (name -> port number).
              Populated automatically from process port allocation.

              Only used when type = "process".
            '';
          };

          listen = lib.mkOption {
            type = types.listOf listenType;
            default = [ ];
            description = ''
              Socket activation configuration for systemd-style socket passing.

              Only used when type = "process".
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

              Only used when type = "process".
            '';
            example = lib.literalExpression ''
              {
                usec = 30000000; # 30 seconds
                require_ready = true;
              }
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

                    Only used when type = "process".
                  '';
                };

                extensions = lib.mkOption {
                  type = types.listOf types.str;
                  default = [ ];
                  description = ''
                    File extensions to watch (e.g., "rs", "js", "py").
                    If empty, all file extensions are watched.

                    Only used when type = "process".
                  '';
                };

                ignore = lib.mkOption {
                  type = types.listOf types.str;
                  default = [ ];
                  description = ''
                    Glob patterns to ignore (e.g., ".git", "target", "*.log").

                    Only used when type = "process".
                  '';
                };
              };
            };
            default = { };
            description = ''
              File watching configuration for automatic process restarts.

              Only used when type = "process".
            '';
          };

        };
      });
  tasksJSON = (lib.mapAttrsToList (name: value: { inherit name; } // value.config) config.tasks);
in
{
  options = {
    tasks = lib.mkOption {
      type = types.attrsOf taskType;
      description = "A set of tasks.";
    };

    task.config = lib.mkOption {
      type = types.package;
      internal = true;
      description = "The generated tasks.json file.";
    };
    task.package = lib.mkOption {
      type = types.nullOr types.package;
      internal = true;
      # CLI 2.0+ runs tasks via Rust, so devenv-tasks binary is not needed for shell entry.
      # However, processes still need the binary for task-based process management.
      # When cli.version is null (flakes integration), always use the binary.
      default =
        if config.devenv.cli.version != null && lib.versionAtLeast config.devenv.cli.version "2.0" && config.processes == { }
        then null
        else lib.getBin devenv-tasks;
    };
  };

  config = {
    assertions = [
      {
        assertion = lib.all (task: task.package.meta.mainProgram == "bash" || task.binary == "bash" || task.exports == [ ]) (lib.attrValues config.tasks);
        message = "The 'exports' option for a task can only be set when 'package' is a bash package.";
      }
      {
        assertion = lib.all (task: task.status == null || task.execIfModified == [ ]) (lib.attrValues config.tasks);
        message = "The 'status' and 'execIfModified' options cannot be used together. Use only one of them to determine whether a task should run.";
      }
    ];

    env.DEVENV_TASKS = builtins.toJSON tasksJSON;
    env.DEVENV_TASK_FILE = config.task.config;
    task.config = (pkgs.formats.json { }).generate "tasks.json" tasksJSON;

    infoSections."tasks" =
      lib.mapAttrsToList
        (name: task: "${name}: ${task.description} (${if task.command == null then "no command" else task.command})")
        config.tasks;

    tasks = {
      "devenv:enterShell" = {
        description = "Runs when entering the shell";
        exec = ''
          mkdir -p "$DEVENV_DOTFILE" || { echo "Failed to create $DEVENV_DOTFILE"; exit 1; }
          # Remove first in case file is owned by another user (chmod would fail otherwise)
          rm -f "$DEVENV_DOTFILE/load-exports" 2>/dev/null || true
          echo "$DEVENV_TASK_ENV" > "$DEVENV_DOTFILE/load-exports"
          chmod +x "$DEVENV_DOTFILE/load-exports"
        '';
      };
      "devenv:enterTest" = {
        description = "Runs when entering the test environment";
        after = [ "devenv:enterShell" ];
      };
    };
    # In devenv 2.0+, Rust runs enterShell tasks before shell spawns (with TUI progress).
    # When cli.version is null (flakes integration) or pre-2.0, run tasks via bash hook.
    enterShell = lib.mkIf (config.devenv.cli.version == null || lib.versionOlder config.devenv.cli.version "2.0") ''
      if [ -z "''${DEVENV_SKIP_TASKS:-}" ]; then
        ${config.task.package}/bin/devenv-tasks run devenv:enterShell --mode all --cache-dir ${lib.escapeShellArg config.devenv.dotfile} --runtime-dir ${lib.escapeShellArg config.devenv.runtime} || exit $?
        if [ -f "$DEVENV_DOTFILE/load-exports" ]; then
          source "$DEVENV_DOTFILE/load-exports"
        fi
      fi
    '';
    # In devenv 2.0+, Rust runs enterTest tasks (with TUI progress).
    # When cli.version is null (flakes integration) or pre-2.0, run tasks via bash hook.
    enterTest = lib.mkIf (config.devenv.cli.version == null || lib.versionOlder config.devenv.cli.version "2.0") (lib.mkBefore ''
      ${config.task.package}/bin/devenv-tasks run devenv:enterTest --mode all --cache-dir ${lib.escapeShellArg config.devenv.dotfile} --runtime-dir ${lib.escapeShellArg config.devenv.runtime} || exit $?
      if [ -f "$DEVENV_DOTFILE/load-exports" ]; then
        source "$DEVENV_DOTFILE/load-exports"
      fi
    '');
  };
}
