{
  pkgs,
  self,
  lib,
  config,
  inputs,
  ...
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
      lib.types.submoduleWith {
        modules = [
          (git-hooks + "/modules/all-modules.nix")
          {
            rootSrc = self;
            package = lib.mkDefault pkgs.prek;
            tools = import (git-hooks + "/nix/call-tools.nix") pkgs;
          }
        ];
        specialArgs = { inherit pkgs; };
        shorthandOnlyDefinesConfig = true;
      }
    else
      defaultModule;

  # Python-based hook runners (e.g. pre-commit) leak their propagatedBuildInputs
  # into PATH via their wrapper script, which prepends a bare Python interpreter
  # that shadows the user's venv/devenv python.
  # Re-wrap without --prefix PATH so only PYTHONPATH is set.
  # Tracking: https://github.com/NixOS/nixpkgs/issues/302376
  package =
    if cfg.package ? dontWrapPythonPrograms then
      cfg.package.overrideAttrs {
        dontWrapPythonPrograms = true;
        postFixup = ''
          buildPythonPath "$out $pythonPath"
          wrapProgramShell $out/bin/${cfg.package.meta.mainProgram} \
            --set PYTHONPATH "$program_PYTHONPATH" \
            --set PYTHONNOUSERSITE true \
            --suffix PATH : ${lib.makeBinPath [ cfg.gitPackage ]}
        '';
      }
    else
      cfg.package;

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
    {
      changelogs = [
        {
          date = "2026-02-02";
          title = "git-hooks.package is now pkgs.prek";
          when = cfg.enable;
          description = ''
            The default package for git-hooks has been changed from `pkgs.pre-commit` to `pkgs.prek`.
          '';
        }
      ];
    }
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
      packages = lib.mkAfter ([ package ] ++ (cfg.enabledPackages or [ ]));
      env.PREK_HOME = "${config.devenv.state}/prek";
      enterShell = lib.mkAfter ''
        mkdir -p "$PREK_HOME"
      '';

      tasks = {
        "devenv:git-hooks:install" = {
          exec =
            let
              executable = lib.getExe package;
              git = lib.getExe cfg.gitPackage;
              configPath = cfg.configPath;
              installStages = cfg.installStages;
            in
            ''
              if ! ${git} rev-parse --git-dir &> /dev/null; then
                echo 1>&2 "WARNING: git-hooks.nix: .git not found; skipping hook installation."
                exit 0
              fi

              # git-hooks installation sets core.hooksPath to .git/hooks
              # which doesn't work with prek (https://github.com/j178/prek/pull/1692), so unset it
              if [ "$(${git} config --get core.hooksPath 2>/dev/null)" = ".git/hooks" ]; then
                ${git} config --unset core.hooksPath
              fi

              # Install hooks for configured stages
              if [ -z "${lib.concatStringsSep " " installStages}" ]; then
                # Default: install pre-commit hook
                ${executable} install -c ${configPath}
              else
                for stage in ${lib.concatStringsSep " " installStages}; do
                  case $stage in
                    manual)
                      # Skip manual stage - it's not a git hook
                      ;;
                    commit|merge-commit|push)
                      ${executable} install -c ${configPath} -t "pre-$stage"
                      ;;
                    *)
                      ${executable} install -c ${configPath} -t "$stage"
                      ;;
                  esac
                done
              fi
            '';
          after = [ "devenv:files" ];
          before = [ "devenv:enterShell" ];
        };
        "devenv:git-hooks:run" = {
          exec = "${lib.getExe package} run -a";
          after = [ "devenv:git-hooks:install" ];
          before = [ "devenv:enterTest" ];
        };
      };
    })

    # Use the files API to manage the pre-commit config file
    (lib.mkIf (cfg.enable && git-hooks != null) {
      files.${cfg.configPath}.source = cfg.configFile;
    })
  ];
}
