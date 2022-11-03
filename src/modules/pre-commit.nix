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
          excludes = [ ".devenv.flake.nix" ];
        }
      ];
      specialArgs = { inherit pkgs; };
      shorthandOnlyDefinesConfig = true;
    };
    default = { };
  };

  config = lib.mkIf ((lib.filterAttrs (id: value: value.enable) config.pre-commit.hooks) != { }) {
    ci = [ config.pre-commit.run ];
    enterShell = config.pre-commit.installationScript;
  };
}
