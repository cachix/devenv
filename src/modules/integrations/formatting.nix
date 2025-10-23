{ pkgs
, lib
, config
, ...
}:
let
  cfg = config.formatting;

  treefmt-nix = config.lib.getInput {
    name = "treefmt-nix";
    url = "github:numtide/treefmt-nix";
    attribute = "treefmt.module";
    follows = [ "nixpkgs" ];
  };
in
{
  options.formatting = {
    enable = lib.mkOption {
      type = lib.types.bool;
      description = "Whether to enable treefmt integration.";
      default = false;
    };

    treefmt = lib.mkOption {
      type = treefmt-nix.lib.submoduleWith lib {
        specialArgs = { inherit pkgs; };
      };
      default = { };
      description = "Integration of https://github.com/numtide/treefmt-nix";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.treefmt.build.wrapper
    ];

    #automatically add treefmt-nix to git-hooks if the user enables it.
    git-hooks.hooks.treefmt.package = lib.mkForce cfg.treefmt.build.wrapper;
  };
}
