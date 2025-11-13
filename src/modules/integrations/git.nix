{ pkgs, config, lib, ... }:

{
  options.git = {
    root = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      description = "Git repository root path. This field is populated automatically in devenv 1.10 and newer.";
      default = null;
    };

    diffTool = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      description = "Git diff tool to use (e.g., 'difftastic', 'vimdiff').";
      default = null;
    };

    mergeTool = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      description = "Git merge tool to use (e.g., 'vimdiff', 'meld').";
      default = null;
    };

    mergeDriver = lib.mkOption {
      type = lib.types.attrsOf (lib.types.submodule {
        options = {
          name = lib.mkOption {
            type = lib.types.str;
            description = "Name of the merge driver.";
          };
          driver = lib.mkOption {
            type = lib.types.str;
            description = "Driver command to run.";
          };
        };
      });
      description = "Custom git merge drivers (e.g., mergiraf).";
      default = { };
    };

    conflictStyle = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      description = "Git conflict style (e.g., 'merge', 'diff3', 'zdiff3').";
      default = null;
    };
  };

  config = lib.mkIf (config.git.diffTool != null || config.git.mergeTool != null || config.git.mergeDriver != { } || config.git.conflictStyle != null) {
    env.GIT_CONFIG_GLOBAL = lib.concatStringsSep "\n" (
      [ "[include]" "    path = ~/.gitconfig" ] ++
      lib.optionals (config.git.diffTool != null) [
        "[diff]"
        "    tool = ${config.git.diffTool}"
      ] ++
      lib.optionals (config.git.conflictStyle != null) [
        "[diff]"
        "    conflictStyle = ${config.git.conflictStyle}"
      ] ++
      lib.optionals (config.git.mergeTool != null) [
        "[merge]"
        "    tool = ${config.git.mergeTool}"
      ] ++
      lib.concatMap
        (driverName: [
          "[merge \"${driverName}\"]"
          "    name = ${config.git.mergeDriver.${driverName}.name}"
          "    driver = ${config.git.mergeDriver.${driverName}.driver}"
        ])
        (lib.attrNames config.git.mergeDriver)
    );
  };
}
