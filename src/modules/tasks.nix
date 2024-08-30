{ pkgs, lib, config, ... }:
let
  types = lib.types;
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
              depends = config.depends;
              command = config.command;
            };
          };
          description = lib.mkOption {
            type = types.str;
            default = "";
            description = "Description of the task.";
          };
          depends = lib.mkOption {
            type = types.listOf types.str;
            description = "List of tasks to run before this task.";
            default = [ ];
          };
        };
      });
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
    info.infoSections.tasks =
      lib.mapAttrsToList
        (name: task: "${name}: ${task.description} ${task.command}")
        config.tasks;

    task.config = (pkgs.formats.json { }).generate "tasks.json"
      (lib.mapAttrsToList (name: value: { inherit name; } // value.config) config.tasks);
    tasks = {
      "devenv:enterShell" = {
        description = "Runs when entering the shell";
      };
      "devenv:enterTest" = {
        description = "Runs when entering the test environment";
      };
    };
    #enterShell = "devenv tasks run devenv:enterShell";
    #enterTest = "devenv tasks run devenv:enterTest";
  };
}
