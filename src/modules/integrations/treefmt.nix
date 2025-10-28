{
  pkgs,
  lib,
  config,
  ...
}:
let
  cfg = config.treefmt;

  treefmt-nix = config.lib.getInput {
    name = "treefmt-nix";
    url = "github:numtide/treefmt-nix";
    attribute = "treefmt";
    follows = [ "nixpkgs" ];
  };

  treefmtSubmodule =
    let
      optionalModule = builtins.tryEval (
        treefmt-nix.lib.submoduleWith lib {
          specialArgs = { inherit pkgs; };
        }
      );
    in
    if optionalModule.success then optionalModule.value else lib.types.deferredModule;

  # Determine tree root: prefer git.root, fallback to devenv.root
  treeRoot = if config.git.root != null then config.git.root else config.devenv.root;

  # Custom wrapper that uses git.root with fallback to devenv.root
  treefmtWrapper = pkgs.writeShellScriptBin "treefmt" ''
    exec ${cfg.config.build.wrapper}/bin/treefmt --config-file ${cfg.config.build.configFile} "$@" --tree-root ${lib.escapeShellArg treeRoot}
  '';
in
{
  options.treefmt = {
    enable = lib.mkEnableOption "treefmt integration (through treefmt-nix)";

    config = lib.mkOption {
      description = "treefmt configuration.";
      type = treefmtSubmodule;
      default = { };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      treefmtWrapper
    ];

    # Automatically add treefmt-nix to git-hooks if the user enables it.
    git-hooks.hooks.treefmt.package = treefmtWrapper;
  };
}
