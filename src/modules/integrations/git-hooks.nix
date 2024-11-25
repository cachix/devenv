{ pkgs, self, lib, config, inputs, ... }:

let
  git-hooks-module =
    inputs.git-hooks
      or inputs.pre-commit-hooks
      or (throw "git-hooks or pre-commit-hooks input required");
in
{
  imports = [
    (lib.mkAliasOptionModule [ "pre-commit" ] [ "git-hooks" ])
  ];

  options.git-hooks = lib.mkOption {
    type = lib.types.submoduleWith {
      modules = [
        (git-hooks-module + "/modules/all-modules.nix")
        {
          rootSrc = self;
          package = pkgs.pre-commit;
          tools = import (git-hooks-module + "/nix/call-tools.nix") pkgs;
        }
      ];
      specialArgs = { inherit pkgs; };
      shorthandOnlyDefinesConfig = true;
    };
    default = { };
    description = "Integration with https://github.com/cachix/git-hooks.nix";
  };

  config = lib.mkIf ((lib.filterAttrs (id: value: value.enable) config.git-hooks.hooks) != { }) {
    ci = [ config.git-hooks.run ];
    # Add the packages for any enabled hooks at the end to avoid overriding the language-defined packages.
    packages = lib.mkAfter ([ config.git-hooks.package ] ++ (config.git-hooks.enabledPackages or [ ]));
    tasks = {
      # TODO: split installation script into status + exec
      "devenv:git-hooks:install" = {
        exec = config.git-hooks.installationScript;
        before = [ "devenv:enterShell" ];
      };
      "devenv:git-hooks:run" = {
        exec = "pre-commit run -a";
        before = [ "devenv:enterTest" ];
      };
    };
  };
}
