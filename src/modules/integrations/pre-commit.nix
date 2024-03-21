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
    enterTest = ''
      pre-commit run -a
    '';
    packages = [ config.pre-commit.package ] ++ (config.pre-commit.enabledPackages or [ ]);
    enterShell = config.pre-commit.installationScript;
  };
}
