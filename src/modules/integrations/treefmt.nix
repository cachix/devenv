{ pkgs
, lib
, config
, ...
}:
let
  cfg = config.treefmt;

  inherit (lib) types;

  treefmt-nix = config.lib.getInput {
    name = "treefmt-nix";
    url = "github:numtide/treefmt-nix";
    attribute = "treefmt";
    follows = [ "nixpkgs" ];
  };

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
      type = treefmt-nix.lib.submoduleWith lib {
        specialArgs = { inherit pkgs; };
      };
      default = { };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      treefmtWrapper
    ];

    #automatically add treefmt-nix to git-hooks if the user enables it.
    git-hooks.hooks.treefmt.package = treefmtWrapper;
  };
}
