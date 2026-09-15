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

  # Absolute config path (quoted for shell use): git runs hooks from the
  # repository toplevel, which differs from the devenv root when devenv
  # lives in a subdirectory.
  configArg = ''"$DEVENV_ROOT/${cfg.configPath}"'';

  stageHookName = stage:
    if stage == "manual" then null
    else if builtins.elem stage [ "commit" "merge-commit" "push" ] then "pre-${stage}"
    else stage;

  gitVersion = cfg.gitPackage.version or null;

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
              tools = lib.mapAttrs (_: lib.mkOptionDefault) (import (git-hooks + "/nix/call-tools.nix") pkgs);
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
          date = "2026-09-04";
          title = "git-hooks are reinstalled only when something changed";
          when = cfg.enable;
          description = ''
            Entering the shell no longer reinstalls the git hooks every time.
            The install task now skips the hook runner when the configuration, the hook runner, and the hooks directory match the previous installation.
            The unchanged case takes 15 ms, down from 26 ms with `prek` and 160 ms with the Python `pre-commit`.
          '';
        }
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
        {
          assertion = !cfg.enable || gitVersion == null || lib.versionAtLeast gitVersion "2.31";
          message = "git-hooks.gitPackage must be Git 2.31 or newer, because the install task uses `git rev-parse --path-format`. Found version ${toString gitVersion}.";
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
              installStages = cfg.installStages;
              expectedHooks = lib.unique (
                if installStages == [ ] then [ "pre-commit" ]
                else builtins.filter (hook: hook != null) (map stageHookName installStages)
              );
              managedConfig = toString cfg.configFile;
              staticInstallKey = builtins.hashString "sha256" (builtins.toJSON {
                version = 3;
                toolExecutable = executable;
                gitExecutable = git;
                configFile = managedConfig;
                inherit expectedHooks;
              });
              installHook = hook: ''"$tool_executable" install -c "$config_path" -t ${lib.escapeShellArg hook}'';
            in
            ''
              set -euo pipefail

              config_path=${configArg}
              managed_config=${lib.escapeShellArg managedConfig}
              tool_executable=${lib.escapeShellArg executable}
              git_executable=${lib.escapeShellArg git}
              static_key=${lib.escapeShellArg staticInstallKey}
              state_dir="$DEVENV_STATE/git-hooks-install"
              key_file="$state_dir/key"
              manifest_file="$state_dir/hooks"

              # The absolute hooks path is the only repository-derived input to
              # the installed hook. One query covers ordinary repositories,
              # linked worktrees, submodules, bare repositories, and core.hooksPath.
              if ! hooks_dir="$($git_executable rev-parse --path-format=absolute --git-path hooks 2>&1)"; then
                echo 1>&2 "WARNING: git-hooks.nix: skipping hook installation: $hooks_dir"
                exit 0
              fi

              # files.nix normally links the generated config directly to this
              # immutable store file, making the common check process-free. Keep
              # hashing as a fallback for a user-replaced regular file.
              if [ "$config_path" -ef "$managed_config" ]; then
                config_fingerprint="managed:$managed_config"
              elif [ -f "$config_path" ]; then
                config_fingerprint="file:$($git_executable hash-object -- "$config_path")"
              else
                echo 1>&2 "devenv: git-hooks config is missing: $config_path"
                exit 1
              fi

              installation_key="$static_key"$'\n'"$hooks_dir"$'\n'"$config_path"$'\n'"$config_fingerprint"

              hook_fingerprint() {
                local hook_path="$1"
                local content target

                if [ -L "$hook_path" ]; then
                  target="$(readlink "$hook_path")"
                  current_fingerprint="symlink:$target"
                elif [ -f "$hook_path" ]; then
                  content="$($git_executable hash-object -- "$hook_path")"
                  current_fingerprint="file:$content"
                else
                  return 1
                fi

                if [ ! -x "$hook_path" ]; then
                  return 1
                fi
              }

              build_manifest() {
                local hook
                local -a manifest_lines=()
                current_manifest=
                for hook in ${lib.escapeShellArgs expectedHooks}; do
                  if ! hook_fingerprint "$hooks_dir/$hook"; then
                    return 1
                  fi
                  manifest_lines+=("$hook $current_fingerprint")
                done
                if [ "''${#manifest_lines[@]}" -gt 0 ]; then
                  printf -v current_manifest '%s\n' "''${manifest_lines[@]}"
                  current_manifest="''${current_manifest%$'\n'}"
                fi
              }

              saved_key=
              saved_manifest=
              [ ! -f "$key_file" ] || saved_key="$(<"$key_file")"
              [ ! -f "$manifest_file" ] || saved_manifest="$(<"$manifest_file")"
              if [ "$saved_key" = "$installation_key" ]; then
                if build_manifest && [ "$saved_manifest" = "$current_manifest" ]; then
                  echo "devenv: git-hooks install cache decision=hit"
                  exit 0
                fi
                cache_decision=stale_hook
              else
                cache_decision=miss
              fi
              echo "devenv: git-hooks install cache decision=$cache_decision"

              mkdir -p "$state_dir"
              cleanup_git_hooks_cache() {
                rm -f "''${new_manifest:-}" "''${new_key:-}"
              }
              trap cleanup_git_hooks_cache EXIT

              ${lib.concatMapStringsSep "\n" installHook expectedHooks}

              new_manifest="$(mktemp "$state_dir/hooks.new.XXXXXX")"
              if ! build_manifest; then
                echo 1>&2 "devenv: git-hooks installer did not create every expected hook"
                exit 1
              fi
              printf '%s\n' "$current_manifest" > "$new_manifest"

              # Publish the key last so an interrupted or failed installation can
              # never turn a partial state update into a cache hit.
              new_key="$(mktemp "$state_dir/key.new.XXXXXX")"
              printf '%s\n' "$installation_key" > "$new_key"
              mv "$new_manifest" "$manifest_file"
              new_manifest=
              mv "$new_key" "$key_file"
              new_key=
            '';
          after = [ "devenv:files" ];
          before = [ "devenv:enterShell" ];
        };
        "devenv:git-hooks:run" = {
          exec = "${lib.getExe package} run -a -c ${configArg}";
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
