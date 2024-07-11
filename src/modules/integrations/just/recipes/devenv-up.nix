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

  config.just.recipes.up = lib.mkIf config.just.enable {
    enable = lib.mkDefault true;
    justfile = lib.mkDefault ''
      # Starts processes in foreground. See http://devenv.sh/processes
      up:
        devenv up
    '';
  };
}
