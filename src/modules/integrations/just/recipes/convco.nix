{ pkgs, lib, config, ... }:

let
  inherit (lib) types mkOption mkEnableOption;
  inherit (import ../utils.nix { inherit lib pkgs; }) recipeModule recipeType;

in
{
  options.just.recipes.convco = mkOption {
    description = "Add the 'changelog' target calling convco";
    type = types.submodule {
      imports = [ recipeModule ];
      options.settings = {
        file-name =
          mkOption {
            type = types.str;
            description = lib.mdDoc "The name of the file to output the chaneglog to.";
            default = "CHANGELOG.md";
          };
      };
    };
  };

  config.just.recipes.convco = {
    package = pkgs.convco;
    justfile =
      let
        binPath = lib.getExe config.just.recipes.convco.package;
        fileName = config.just.recipes.convco.settings.file-name;
      in
      ''
        # Generate ${fileName} using recent commits
        changelog:
          ${binPath} changelog -p "" > ${fileName}
      '';
  };
}
