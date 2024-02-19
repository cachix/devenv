{ pkgs, self, lib, pre-commit-hooks, config, ... }:

{
  options.pre-commit = lib.mkOption {
    type = lib.types.submoduleWith {
      modules = [
        (pre-commit-hooks + "/modules/all-modules.nix")
        {
          rootSrc = self;
          package = pkgs.pre-commit;
          tools = import (pre-commit-hooks + "/nix/call-tools.nix") pkgs;
        }
      ];
      specialArgs = { inherit pkgs; };
      shorthandOnlyDefinesConfig = true;
    };
    default = { };
    description = "Integration of https://github.com/cachix/pre-commit-hooks.nix";
  };

  config = lib.mkIf ((lib.filterAttrs (id: value: value.enable) config.pre-commit.hooks) != { }) {
    ci = [ config.pre-commit.run ];
    packages = [ config.pre-commit.package ];
    enterShell = config.pre-commit.installationScript;
  };
}
