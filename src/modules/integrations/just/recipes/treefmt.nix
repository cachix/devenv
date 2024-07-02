{ pkgs, lib, config, ... }:

let
  inherit (lib) types mkOption mkEnableOption;
  inherit (import ../utils.nix { inherit lib pkgs; }) recipeModule recipeType;

in
{
  options.just.recipes.treefmt = mkOption {
    description = "Add the 'fmt' target to format source tree using treefmt";
    type = types.submodule {
      imports = [ recipeModule ];
    };
  };

  config.just.recipes.treefmt = {
    justfile = ''
      # Auto-format the source tree using treefmt
      fmt:
        treefmt
    '';
  };
}
