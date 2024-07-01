{ pkgs, self, lib, inputs, config, ... }:
let

in {

  options.treefmt = lib.mkOption {
    type = lib.types.submoduleWith {
      modules = inputs.treefmt-nix.lib.submodule-modules;
      specialArgs = { inherit pkgs; };
      shorthandOnlyDefinesConfig = true;
    };
    default = { };
    description = "Integration of https://github.com/numtide/treefmt-nix";
  };

  config = lib.mkIf ((lib.filterAttrs (id: value: value.enable) config.treefmt.programs) != { }) {
    packages = [
      config.treefmt.build.wrapper
    ];

    #automatically add treefmt-nix to pre-commit if the user enables it.
    pre-commit.hooks.treefmt.package = config.treefmt.build.wrapper;
  };
}
