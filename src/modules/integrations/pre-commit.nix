{ pkgs, self, lib, git-hooks, config, ... }:

{
  options.pre-commit = lib.mkOption {
    type = lib.types.submoduleWith {
      modules = [
        (git-hooks + "/modules/all-modules.nix")
        {
          rootSrc = self;
          package = pkgs.pre-commit;
          tools = import (git-hooks + "/nix/call-tools.nix") pkgs;
        }
      ];
      specialArgs = { inherit pkgs; };
      shorthandOnlyDefinesConfig = true;
    };
    default = { };
    description = "Integration of https://github.com/cachix/git-hooks.nix";
  };

  config = lib.mkIf ((lib.filterAttrs (id: value: value.enable) config.pre-commit.hooks) != { }) {
    ci = [ config.pre-commit.run ];
    enterTest = ''
      pre-commit run -a
    '';
    # Add the packages for any enabled hooks at the end to avoid overriding the language-defined packages.
    packages = lib.mkAfter ([ config.pre-commit.package ] ++ (config.pre-commit.enabledPackages or [ ]));
    enterShell = config.pre-commit.installationScript;
  };
}
