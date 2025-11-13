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
  };

  config = lib.mkIf (config.git.diffTool != null || config.git.mergeTool != null) {
    env.GIT_CONFIG_GLOBAL = lib.concatStringsSep "\n" (
      [ "[include]" "    path = ~/.gitconfig" ] ++
      lib.optionals (config.git.diffTool != null) [
        "[diff]"
        "    tool = ${config.git.diffTool}"
      ] ++
      lib.optionals (config.git.mergeTool != null) [
        "[merge]"
        "    tool = ${config.git.mergeTool}"
      ]
    );
  };
}
