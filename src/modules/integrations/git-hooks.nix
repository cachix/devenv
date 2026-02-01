{ pkgs
, self
, lib
, config
, inputs
, ...
}:

let
  cfg = config.git-hooks;

  inputArgs = {
    name = "git-hooks";
    url = "github:cachix/git-hooks.nix";
    attribute = "git-hooks";
    follows = [ "nixpkgs" ];
  };

  git-hooks = inputs.git-hooks or inputs.pre-commit-hooks or (config.lib.tryGetInput inputArgs);

  # Check if any individual hooks are enabled
  anyHookEnabled = builtins.any (hook: hook.enable or false) (lib.attrValues (cfg.hooks or { }));

  # A default module stub for when git-hooks is not available.
  # Uses freeformType to accept any attributes (tools, hooks, etc.) without type errors.
  defaultModule = lib.types.submoduleWith {
    modules = [
      (
        { ... }:
        {
          freeformType = lib.types.attrsOf lib.types.anything;
          options = {
            enable = lib.mkOption {
              type = lib.types.bool;
              description = ''
                Whether to enable the pre-commit hooks module.

                When set to false, this disables the entire module.
              '';
              default = false;
            };
          };
        }
      )
    ];
  };

  githooksSubmodule =
    if git-hooks != null then
      lib.types.submoduleWith
        {
          modules = [
            (git-hooks + "/modules/all-modules.nix")
            {
              rootSrc = self;
              package = lib.mkDefault pkgs.pre-commit;
              tools = import (git-hooks + "/nix/call-tools.nix") pkgs;
            }
          ];
          specialArgs = { inherit pkgs; };
          shorthandOnlyDefinesConfig = true;
        }
    else
      defaultModule;

  # `propagatedBuildInputs` in Python apps are leaked into the environment.
  # This normally leaks the Python interpreter and its site-packages, causing collision errors.
  # This affects all packages built with `buildPythonApplication` or `toPythonApplication`.
  # pre-commit is particularly annoying as it is difficult for end-users to track down.
  # Tracking: https://github.com/NixOS/nixpkgs/issues/302376
  packageBin =
    pkgs.runCommandLocal "pre-commit-bin" { meta.mainProgram = cfg.package.meta.mainProgram; }
      ''
        mkdir -p $out/bin
        ln -s ${lib.getExe cfg.package} $out/bin/${cfg.package.meta.mainProgram}
      '';

in
{
  imports = [
    (lib.mkRenamedOptionModule [ "pre-commit" ] [ "git-hooks" ])
  ];

  options.git-hooks = lib.mkOption {
    type = githooksSubmodule;
    default = { };
    description = "Integration with https://github.com/cachix/git-hooks.nix";
  };

  config = lib.mkMerge [
    # Auto-enable when any hook is enabled, so other modules can check git-hooks.enable
    {
      git-hooks.enable = lib.mkDefault anyHookEnabled;
    }

    # Assert that input is available when hooks are configured
    {
      assertions = [
        {
          assertion = !cfg.enable || git-hooks != null;
          message = config.lib._mkInputError inputArgs;
        }
      ];
    }

    (lib.mkIf cfg.enable {
      ci = [ cfg.run ];
      # Add the packages for any enabled hooks at the end to avoid overriding the language-defined packages.
      packages = lib.mkAfter ([ packageBin ] ++ (cfg.enabledPackages or [ ]));

      # Use the files API to manage the pre-commit config file
      files.${cfg.configPath}.source = cfg.configFile;

      tasks = {
        "devenv:git-hooks:install" = {
          # The config file is managed by the files API.
          # installationScript also creates the symlink, but it will be a no-op
          # since the files API already created it pointing to the same target.
          exec = cfg.installationScript;
          after = [ "devenv:files" ];
          before = [ "devenv:enterShell" ];
        };
        "devenv:git-hooks:run" = {
          exec = "${packageBin.meta.mainProgram} run -a";
          after = [ "devenv:git-hooks:install" ];
          before = [ "devenv:enterTest" ];
        };
      };
    })
  ];
}
