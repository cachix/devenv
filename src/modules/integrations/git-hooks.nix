{ pkgs, self, lib, config, inputs, ... }:

let
  cfg = config.git-hooks;

  git-hooks-module =
    inputs.git-hooks
      or inputs.pre-commit-hooks
      or (throw "git-hooks or pre-commit-hooks input required");

  # `propagatedBuildInputs` in Python apps are leaked into the environment.
  # This normally leaks the Python interpreter and its site-packages, causing collision errors.
  # This affects all packages built with `buildPythonApplication` or `toPythonApplication`.
  # pre-commit is particularly annoying as it is difficult for end-users to track down.
  # Tracking: https://github.com/NixOS/nixpkgs/issues/302376
  packageBin = pkgs.runCommandLocal "pre-commit-bin" { } ''
    mkdir -p $out/bin
    ln -s ${cfg.package}/bin/pre-commit $out/bin/pre-commit
  '';
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

  config = lib.mkIf ((lib.filterAttrs (id: value: value.enable) cfg.hooks) != { }) {
    ci = [ cfg.run ];
    # Add the packages for any enabled hooks at the end to avoid overriding the language-defined packages.
    packages = lib.mkAfter ([ packageBin ] ++ (cfg.enabledPackages or [ ]));
    tasks = {
      # TODO: split installation script into status + exec
      "devenv:git-hooks:install" = {
        exec = cfg.installationScript;
        before = [ "devenv:enterShell" ];
      };
      "devenv:git-hooks:run" = {
        exec = "pre-commit run -a";
        before = [ "devenv:enterTest" ];
      };
    };
  };
}
