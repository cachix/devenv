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
    # Add the packages for any enabled hooks at the end to avoid overriding the language-defined packages.
    packages = lib.mkAfter ([ config.pre-commit.package ] ++ (config.pre-commit.enabledPackages or [ ]));
    tasks = {
      # TODO: split installation script into status + exec
      "devenv:pre-commit:install".exec = config.pre-commit.installationScript;
      "devenv:pre-commit:run".exec = "pre-commit run -a";
      "devenv:enterShell".after = [ "devenv:pre-commit:install" ];
      "devenv:enterTest".after = [ "devenv:pre-commit:run" ];
    };
  };
}
