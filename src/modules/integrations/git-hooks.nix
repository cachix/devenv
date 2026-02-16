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
      # Add the packages for any enabled hooks at the end to avoid overriding the language-defined packages.
      packages = lib.mkAfter ([ packageBin ] ++ (cfg.enabledPackages or [ ]));
      env.PREK_HOME = "${config.devenv.state}/prek";
      enterShell = lib.mkAfter ''
        mkdir -p "$PREK_HOME"
      '';

      tasks = {
        "devenv:git-hooks:install" = {
          # The config file is managed by the files API (see files.${cfg.configPath} below).
          # We write a custom install script here instead of using cfg.installationScript
          # because the upstream script skips installation when the config symlink exists,
          # but with the files API the symlink is created before this task runs.
          exec =
            let
              executable = lib.getExe packageBin;
              git = lib.getExe cfg.gitPackage;
              configPath = cfg.configPath;
              installStages = cfg.installStages;
            in
            ''
              if ! ${git} rev-parse --git-dir &> /dev/null; then
                echo 1>&2 "WARNING: git-hooks.nix: .git not found; skipping hook installation."
                exit 0
              fi

              did_install_hooks=0

              setup_hooks_path() {
                GIT_WC=$(${git} rev-parse --show-toplevel)
                git_dir_abs=$(${git} -C "$GIT_WC" rev-parse --path-format=absolute --git-dir)
                common_dir_abs=$(${git} -C "$GIT_WC" rev-parse --path-format=absolute --git-common-dir)
                common_dir=$(${git} -C "$GIT_WC" rev-parse --path-format=relative --git-common-dir)
                common_is_bare=$(${git} config --file "$common_dir_abs/config" --bool core.bare || true)

                # In linked worktrees, store hooksPath in worktree-local config so
                # each worktree resolves the relative path against its own root.
                config_scope="--local"
                hooks_path="$common_dir/hooks"
                if [ "$git_dir_abs" != "$common_dir_abs" ] && [ "$common_is_bare" != "true" ]; then
                  ${git} config --local extensions.worktreeConfig true
                  ${git} config --local --unset-all core.hooksPath || true
                  config_scope="--worktree"
                elif [ "$git_dir_abs" != "$common_dir_abs" ] && [ "$common_is_bare" = "true" ]; then
                  hooks_path="$common_dir_abs/hooks"
                fi

                ${git} config "$config_scope" core.hooksPath ""
              }

              # Install hooks for configured stages
              if [ -z "${lib.concatStringsSep " " installStages}" ]; then
                did_install_hooks=1
                setup_hooks_path

                # Default: install pre-commit hook
                ${executable} install -c ${configPath}
              else
                for stage in ${lib.concatStringsSep " " installStages}; do
                  case $stage in
                    manual)
                      # Skip manual stage - it's not a git hook
                      ;;
                    commit|merge-commit|push)
                      if [ "$did_install_hooks" -eq 0 ]; then
                        did_install_hooks=1
                        setup_hooks_path
                      fi

                      ${executable} install -c ${configPath} -t "pre-$stage"
                      ;;
                    *)
                      if [ "$did_install_hooks" -eq 0 ]; then
                        did_install_hooks=1
                        setup_hooks_path
                      fi

                      ${executable} install -c ${configPath} -t "$stage"
                      ;;
                  esac
                done
              fi

              if [ "$did_install_hooks" -eq 1 ]; then
                ${git} config "$config_scope" core.hooksPath "$hooks_path"
              fi
            '';
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

    # Use the files API to manage the pre-commit config file
    (lib.mkIf (cfg.enable && git-hooks != null) {
      files.${cfg.configPath}.source = cfg.configFile;
    })
  ];
}
