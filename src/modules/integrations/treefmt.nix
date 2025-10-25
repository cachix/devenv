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
      cfg.config.build.wrapper
    ];

    #automatically add treefmt-nix to git-hooks if the user enables it.
    git-hooks.hooks.treefmt.package = cfg.config.build.wrapper;
  };
}
