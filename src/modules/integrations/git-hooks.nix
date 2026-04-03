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
      cfg.package.overrideAttrs
        {
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
              realpath = "${pkgs.coreutils}/bin/realpath";
            in
            ''
              if ! ${git} rev-parse --git-dir &> /dev/null; then
                echo 1>&2 "WARNING: git-hooks.nix: .git not found; skipping hook installation."
                exit 0
              fi

              managed_hooks_scope=""
              managed_hooks_path=""
              restore_hooks_path=0
              restore_worktree_config=0
              previous_worktree_config_set=0
              previous_worktree_config=""
              target_hooks_scope=""
              target_hooks_path=""
              target_hooks_abs=""
              did_prepare_install=0

              restore_git_hooks_config() {
                if [ "$restore_hooks_path" -eq 1 ]; then
                  ${git} config "$managed_hooks_scope" core.hooksPath "$managed_hooks_path" || true
                fi

                if [ "$restore_worktree_config" -eq 1 ]; then
                  if [ "$previous_worktree_config_set" -eq 1 ]; then
                    ${git} config --local extensions.worktreeConfig "$previous_worktree_config" || true
                  else
                    ${git} config --local --unset-all extensions.worktreeConfig || true
                  fi
                fi
              }

              prepare_hook_install() {
                if [ "$did_prepare_install" -eq 1 ]; then
                  return
                fi
                did_prepare_install=1

                GIT_DIR_ABS=$(${git} rev-parse --path-format=absolute --git-dir)
                COMMON_DIR_ABS=$(${git} rev-parse --path-format=absolute --git-common-dir)
                DEFAULT_HOOKS_ABS=$(${realpath} "$GIT_DIR_ABS/hooks" 2>/dev/null || printf '%s\n' "$GIT_DIR_ABS/hooks")

                # TODO: Drop this linked-worktree wrapper once prek handles
                # repo/worktree-scoped core.hooksPath itself (j178/prek#1673).
                if [ "$GIT_DIR_ABS" != "$COMMON_DIR_ABS" ]; then
                  COMMON_IS_BARE=$(${git} config --file "$COMMON_DIR_ABS/config" --bool core.bare || true)
                  if [ "$COMMON_IS_BARE" = "true" ]; then
                    target_hooks_scope="--local"
                    target_hooks_path="$COMMON_DIR_ABS/hooks"
                  else
                    target_hooks_scope="--worktree"
                    target_hooks_path="$(${git} rev-parse --path-format=relative --git-common-dir)/hooks"

                    if previous_worktree_config=$(${git} config --local --bool --get extensions.worktreeConfig 2>/dev/null); then
                      previous_worktree_config_set=1
                    fi

                    if [ "$previous_worktree_config_set" -eq 0 ] || [ "$previous_worktree_config" != "true" ]; then
                      ${git} config --local extensions.worktreeConfig true
                      restore_worktree_config=1
                    fi
                  fi

                  target_hooks_abs=$(${realpath} "$target_hooks_path" 2>/dev/null || printf '%s\n' "$target_hooks_path")
                fi

                # Allow install to proceed only when core.hooksPath already points
                # at a hooks path that devenv manages itself:
                # 1. The repository's default hooks dir (for repeated installs or
                #    absolute default paths inside submodules)
                # 2. The linked-worktree hooksPath that this wrapper restores
                hooks_scope_and_path=$(${git} config --show-scope --get core.hooksPath 2>/dev/null || true)
                if [ -n "$hooks_scope_and_path" ]; then
                  tab=$(printf '\t')
                  current_hooks_scope=''${hooks_scope_and_path%%"$tab"*}
                  current_hooks_path=''${hooks_scope_and_path#*"$tab"}

                  if [ "$current_hooks_scope" = "local" ] || [ "$current_hooks_scope" = "worktree" ]; then
                    current_hooks_abs=$(${realpath} "$current_hooks_path" 2>/dev/null || printf '%s\n' "$current_hooks_path")
                    should_unset_hooks_path=0

                    if [ "$current_hooks_abs" = "$DEFAULT_HOOKS_ABS" ]; then
                      should_unset_hooks_path=1
                    elif [ -n "$target_hooks_scope" ] &&
                      [ "--$current_hooks_scope" = "$target_hooks_scope" ] &&
                      [ "$current_hooks_abs" = "$target_hooks_abs" ]; then
                      should_unset_hooks_path=1
                    fi

                    if [ "$should_unset_hooks_path" -eq 1 ]; then
                      managed_hooks_scope="--$current_hooks_scope"
                      managed_hooks_path="$current_hooks_path"
                      restore_hooks_path=1
                      ${git} config "$managed_hooks_scope" --unset-all core.hooksPath
                    fi
                  fi
                fi
              }

              trap restore_git_hooks_config EXIT

              # Install hooks for configured stages
              if [ -z "${lib.concatStringsSep " " installStages}" ]; then
                prepare_hook_install

                # Default: install pre-commit hook
                ${executable} install -c ${configPath}
              else
                for stage in ${lib.concatStringsSep " " installStages}; do
                  case $stage in
                    manual)
                      # Skip manual stage - it's not a git hook
                      ;;
                    commit|merge-commit|push)
                      prepare_hook_install
                      ${executable} install -c ${configPath} -t "pre-$stage"
                      ;;
                    *)
                      prepare_hook_install
                      ${executable} install -c ${configPath} -t "$stage"
                      ;;
                  esac
                done
              fi

              restore_hooks_path=0
              if [ -n "$target_hooks_scope" ]; then
                ${git} config "$target_hooks_scope" core.hooksPath "$target_hooks_path"
                restore_worktree_config=0
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
