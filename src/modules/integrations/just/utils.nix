{ lib, pkgs, ... }:
let
  inherit (lib) types;
  inherit (lib.attrsets) recursiveUpdate;

  recipeModule = {
    imports = [ ./recipe-module.nix ];
    config._module.args = { inherit pkgs; };
  };
  recipeType = types.submodule recipeModule;

  mkCmdArgs = predActionList:
    lib.concatStringsSep
      " "
      (builtins.foldl'
        (acc: entry:
          acc ++ lib.optional (builtins.elemAt entry 0) (builtins.elemAt entry 1))
        [ ]
        predActionList);

in
{
  inherit mkCmdArgs recipeModule recipeType;
}
