{ pkgs, lib, config, ... }@inputs:
let
  types = lib.types;

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
              # Only auto-add exec for single-line process commands that don't already have exec.
              # Multi-line scripts need manual exec placement before the main process.
              trimmedCommand = lib.strings.trim command;
              isSingleLine = !lib.hasInfix "\n" trimmedCommand;
              alreadyHasExec = lib.hasPrefix "exec " trimmedCommand;
              addExec = config.type == "process" && isSingleLine && !alreadyHasExec;
            in
            pkgs.writeScript name ''
              #!${binary}
              ${lib.optionalString (!isStatus && isBash) "set -e"}
              ${lib.optionalString addExec "exec "}${command}
              ${lib.optionalString (config.exports != [] && !isStatus) "${inputs.config.task.package}/bin/devenv-tasks export ${lib.concatStringsSep " " config.exports}"}
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
              cwd = config.cwd;
              show_output = config.showOutput;
            };
            description = "Internal configuration for the task.";
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
            '';
            default = [ ];
          };
          before = lib.mkOption {
            type = types.listOf types.str;
            description = ''
              List of tasks that depend on this task completing first.

              Here's a helpful mnemonic to remember: This task runs *before* these tasks.
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
      type = types.package;
      internal = true;
      default = lib.getBin devenv-tasks;
    };
  };

  config = {
    warnings =
      let
        multiLineProcessesWithoutExec = lib.filterAttrs
          (name: task:
            task.type == "process" &&
            task.exec != null &&
            lib.hasInfix "\n" (lib.strings.trim task.exec) &&
            !lib.hasInfix "exec " task.exec
          )
          config.tasks;
      in
      lib.mapAttrsToList
        (name: _:
          "Process '${name}' has a multi-line command without 'exec'. This may cause SIGTERM to not reach the actual process. Consider adding 'exec' before the main process command."
        )
        multiLineProcessesWithoutExec;

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
          echo "$DEVENV_TASK_ENV" > "$DEVENV_DOTFILE/load-exports"
          chmod +x "$DEVENV_DOTFILE/load-exports"
        '';
      };
      "devenv:enterTest" = {
        description = "Runs when entering the test environment";
        after = [ "devenv:enterShell" ];
      };
    };
    enterShell = ''
      if [ -z "''${DEVENV_SKIP_TASKS:-}" ]; then
        ${config.task.package}/bin/devenv-tasks run devenv:enterShell --mode all || exit $?
        if [ -f "$DEVENV_DOTFILE/load-exports" ]; then
          source "$DEVENV_DOTFILE/load-exports"
        fi
      fi
    '';
    enterTest = lib.mkBefore ''
      ${config.task.package}/bin/devenv-tasks run devenv:enterTest --mode all || exit $?
      if [ -f "$DEVENV_DOTFILE/load-exports" ]; then
        source "$DEVENV_DOTFILE/load-exports"
      fi
    '';
  };
}
