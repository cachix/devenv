{ pkgs
, lib
, config
, ...
}:
let
  cfg = config.treefmt;

  inputArgs = {
    name = "treefmt-nix";
    url = "github:numtide/treefmt-nix";
    attribute = "treefmt";
    follows = [ "nixpkgs" ];
  };

  # When enabled, use getInput (throws helpful error if missing)
  # Otherwise, use tryGetInput to populate the docs when the input is available.
  treefmt-nix =
    if cfg.enable then config.lib.getInput inputArgs else config.lib.tryGetInput inputArgs;

  treefmtSubmodule =
    if treefmt-nix != null then
      treefmt-nix.lib.submoduleWith lib
        {
          specialArgs = { inherit pkgs; };
        }
    else
      lib.types.attrs;

  # Determine tree root: prefer git.root, fallback to devenv.root
  treeRoot = if config.git.root != null then config.git.root else config.devenv.root;

  # Custom wrapper to point treefmt to the project root.
  #
  # treefmt-nix provides a `config.build.wrapper` that uses `config.projectRootFile` to find the tree root.
  # We instead set the tree root directory based on the information provided by the CLI.
  # If the user sets a custom `projectRootFile`, we'll use that.
  treefmtWrapper =
    let
      treeRootOption =
        if cfg.config.projectRootFile != "" then
          "--tree-root-file " + lib.escapeShellArg cfg.config.projectRootFile
        else
          "--tree-root " + lib.escapeShellArg treeRoot;
    in
    pkgs.writeShellScriptBin "treefmt" ''
      exec ${cfg.config.package}/bin/treefmt --config-file ${cfg.config.build.configFile} "$@" ${treeRootOption}
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
    # Use the same wrapper for git-hooks.
    git-hooks.hooks.treefmt.package = config.lib.mkOverrideDefault treefmtWrapper;

    packages = [
      treefmtWrapper
    ];

    # We automatically configure treefmt with the correct tree root for the project.
    # Set an empty default to detect when the user wants to use a custom root file.
    treefmt.config.projectRootFile = lib.mkDefault "";

    tasks."devenv:treefmt:run".exec = "${treefmtWrapper}/bin/treefmt";
  };
}
