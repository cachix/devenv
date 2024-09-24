{ pkgs, lib, config, ... }@inputs:
let
  types = lib.types;
  devenv = import ./../../package.nix { inherit pkgs inputs; build_tasks = true; };
  taskType = types.submodule
    ({ name, config, ... }:
      let
        mkCommand = command:
          if builtins.isNull command
          then null
          else
            pkgs.writeScript name ''
              #!${pkgs.lib.getBin config.package}/bin/${config.binary}
              ${command}
              ${lib.optionalString (config.exports != []) "${devenv}/bin/tasks export ${lib.concatStringsSep " " config.exports}"}
            '';
      in
      {
        options = {
          exec = lib.mkOption {
            type = types.nullOr types.str;
            default = null;
            description = "Command to execute the task.";
          };
          binary = lib.mkOption {
            type = types.str;
            description = "Override the binary name if it doesn't match package name";
            default = config.package.pname;
          };
          package = lib.mkOption {
            type = types.package;
            default = pkgs.bash;
            description = "Package to install for this task.";
          };
          command = lib.mkOption {
            type = types.nullOr types.package;
            internal = true;
            default = mkCommand config.exec;
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
            default = mkCommand config.status;
            description = "Path to the script to run.";
          };
          config = lib.mkOption {
            type = types.attrsOf types.anything;
            internal = true;
            default = {
              name = name;
              description = config.description;
              status = config.statusCommand;
              after = config.after;
              command = config.command;
              input = config.input;
            };
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
          after = lib.mkOption {
            type = types.listOf types.str;
            description = "List of tasks to run before this task.";
            default = [ ];
          };
          input = lib.mkOption {
            type = types.attrsOf types.anything;
            default = { };
            description = "Input values for the task, encoded as JSON.";
          };
        };
      });
  tasksJSON = (lib.mapAttrsToList (name: value: { inherit name; } // value.config) config.tasks);
in
{
  options.tasks = lib.mkOption {
    type = types.attrsOf taskType;
  };

  options.task.config = lib.mkOption {
    type = types.package;
    internal = true;
  };

  config = {
    env.DEVENV_TASKS = builtins.toJSON tasksJSON;

    assertions = [
      {
        assertion = lib.all (task: task.binary == "bash" || task.export == [ ]) (lib.attrValues config.tasks);
        message = "The 'export' option can only be set when 'binary' is set to 'bash'.";
      }
    ];

    infoSections."tasks" =
      lib.mapAttrsToList
        (name: task: "${name}: ${task.description} (${if task.command == null then "no command" else task.command})")
        config.tasks;

    task.config = (pkgs.formats.json { }).generate "tasks.json" tasksJSON;

    tasks = {
      "devenv:enterShell" = {
        description = "Runs when entering the shell";
        exec = ''
          echo "$DEVENV_TASK_ENV" > "$DEVENV_DOTFILE/load-exports"
          chmod +x "$DEVENV_DOTFILE/load-exports"
        '';
      };
      "devenv:enterTest" = {
        description = "Runs when entering the test environment";
      };
    };
    enterShell = ''
      ${devenv}/bin/tasks run devenv:enterShell
      if [ -f "$DEVENV_DOTFILE/load-exports" ]; then
        source "$DEVENV_DOTFILE/load-exports"
      fi
    '';
    enterTest = ''
      ${devenv}/bin/tasks run devenv:enterTest
    '';
  };
}
