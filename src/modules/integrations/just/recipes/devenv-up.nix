{ pkgs, lib, config, ... }:

let
  inherit (lib) types mkOption mkEnableOption;
  inherit (import ../utils.nix { inherit lib pkgs; }) recipeModule recipeType;

in
{
  options.just.recipes.up = mkOption {
    description = "Starts processes in foreground. See http://devenv.sh/processes";
    type = types.submodule {
      imports = [ recipeModule ];
    };
  };

  config.just.recipes.up = {
    enable = true;
    justfile = ''
      # Starts processes in foreground. See http://devenv.sh/processes
      up:
        devenv up
    '';
  };
}
