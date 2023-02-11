{ pkgs, lib, config, ... }:

let
  types = lib.types;
  buildModule = types.submodule {
    imports = [ ./build-options.nix ];
  };
in
{
  options = {
    build.default = lib.mkOption {
      type = types.listOf types.str;
      default = [ "shell" ];
      description = "List of builds to invoke when no arguments are passed to `devenv build`.";
    };

    build.derivations = lib.mkOption {
      type = types.lazyAttrsOf types.package;
      internal = true;
    };

    builds = lib.mkOption {
      type = types.submodule {
        freeformType = types.lazyAttrsOf buildModule;
      };
      description = "Attribute names with corresponding options to build.";
    };
  };

  config = {
    builds.shell.derivation = config.shell.inputDerivation;

    build.derivations = builtins.mapAttrs (name: value: value.derivation) config.builds;
  };
}
