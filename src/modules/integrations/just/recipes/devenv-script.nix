{ pkgs, lib, config, ... }:

let
  inherit (lib) types mkOption mkEnableOption;
  inherit (import ../utils.nix { inherit lib pkgs; }) recipeModule recipeType;

  devenvScriptRecipes = lib.genAttrs (builtins.attrNames config.scripts) (name:
    let
      script = config.scripts.${name};
    in
    mkOption {
      description = script.description;
      type = types.submodule {
        imports = [ recipeModule ];
      };
    });

in
{
  options = {
    just = {
      recipes = devenvScriptRecipes;
    };
  };

  config = lib.mkIf config.just.enable {
    just = {
      recipes = lib.genAttrs (builtins.attrNames config.scripts) (name:
        let
          script = config.scripts.${name};
        in
        {
          enable = lib.mkDefault script.just.enable;
          justfile = lib.mkDefault ''
            #${script.description}
            ${name}:
              ${name}
          '';
        });
    };
  };
}
