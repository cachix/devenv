{ pkgs, self, lib, pre-commit-hooks, config, ... }:
let
  inherit (lib.lists) optionals;
in
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
    warnings = optionals ((lib.filterAttrs (id: value: value.enable) config.treefmt.programs) == { }) [
      ''
        You have enabled pre-commit for treefmt but do not have any formatters enabled.
      ''
    ];
    ci = [ config.pre-commit.run ];
    enterTest = ''
      pre-commit run -a
    '';
    # Add the packages for any enabled hooks at the end to avoid overriding the language-defined packages.
    packages = lib.mkAfter ([ config.pre-commit.package ] ++ (config.pre-commit.enabledPackages or [ ]));
    enterShell = config.pre-commit.installationScript;
  };
}
