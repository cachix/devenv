{ pkgs, lib, config, ... }:

let
  inherit (lib) types mkOption mkEnableOption;
  inherit (lib.lists) optionals;
  inherit (import ../utils.nix { inherit lib pkgs; }) recipeModule recipeType;

in
{
  options.just.recipes.treefmt = mkOption {
    description = "Add the 'fmt' target to format source tree using treefmt";
    type = types.submodule {
      imports = [ recipeModule ];
    };
  };

  config = lib.mkIf config.just.enable {
    warnings = optionals ((lib.filterAttrs (id: value: value.enable) config.treefmt.programs) == { }) [
      ''
        You have enabled the Just runner for treefmt but do not have any formatters enabled.
      ''
    ];

    just.recipes.treefmt = {
      package = lib.mkDefault config.treefmt.build.wrapper;
      justfile = lib.mkDefault ''
        # Auto-format the source tree using treefmt
        fmt:
          ${lib.getExe config.just.recipes.treefmt.package}
      '';
    };
  };
}
