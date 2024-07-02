{ pkgs, lib, config, ... }:

let
  inherit (lib) types mkOption mkEnableOption;
  inherit (import ../utils.nix { inherit lib pkgs; }) recipeModule recipeType;

in
{
  options.just.recipes.rust = mkOption {
    description = "Add 'w' and 'test' targets for running cargo";
    type = types.submodule {
      imports = [ recipeModule ];
    };
  };

  config.just.recipes.rust = {
    justfile = ''
      # Compile and watch the project
      w:
        cargo watch

      # Run and watch 'cargo test'
      test:
        cargo watch -s "cargo test"
    '';
  };
}
