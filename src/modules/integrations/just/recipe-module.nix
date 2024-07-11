{ config, name, lib, pkgs, ... }:
let
  inherit (lib) mkOption mkEnableOption types;

in
{
  options = {
    enable = mkEnableOption "this recipe";
    justfile = mkOption {
      type = types.either types.str types.path;
      description = ''
        The justfile representing this recipe.
      '';
      apply = x:
        if builtins.isPath x then x else
        pkgs.writeTextFile {
          name = "${name}.just";
          text = x;
        };
    };
    package = mkOption {
      type = types.nullOr types.package;
      default = null;
      description = lib.mdDoc
        ''
          An optional package that provides the recipe.
        '';
    };
    outputs.justfile = mkOption {
      type = types.str;
      readOnly = true;
      description = ''
        The justfile code for importing this recipe's justfile.

        See https://just.systems/man/en/chapter_53.html
      '';
      default =
        if config.enable
        then "import '${builtins.toString config.justfile}'"
        else "";
    };
  };
}
