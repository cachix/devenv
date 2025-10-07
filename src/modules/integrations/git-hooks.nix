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
    ln -s ${lib.getExe cfg.package} $out/bin/pre-commit-bin
  '';

  anyEnabled = builtins.any (hook: hook.enable) (lib.attrValues cfg.hooks);

  # Store additional state in between evaluations to support uninstalling hooks.
  hookStateDir = "${config.devenv.state}/git-hooks";
  hookStateFile = "${hookStateDir}/config.json";
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "pre-commit" ] [ "git-hooks" ])
  ];

  options.git-hooks = lib.mkOption {
    type = lib.types.submoduleWith {
      modules = [
        (git-hooks-module + "/modules/all-modules.nix")
        {
          rootSrc = self;
          package = lib.mkDefault pkgs.pre-commit;
          tools = import (git-hooks-module + "/nix/call-tools.nix") pkgs;
        }
      ];
      specialArgs = { inherit pkgs; };
      shorthandOnlyDefinesConfig = true;
    };
    default = { };
    description = "Integration with https://github.com/cachix/git-hooks.nix";
  };

  config = lib.mkMerge [
    (lib.mkIf (!anyEnabled) {
      # Remove the existing `configPath` if it exists and is in the nix store
      #
      # TODO(sander): turn this into a task.
      # Introduce a task that only shows up in logs if executed or if running in verbose mode.
      enterShell = ''
        # Read the path to the installed `configPath` from the hook state.
        configFile=""
        if [ -f '${hookStateFile}' ]; then
          prevConfigPath=$(${lib.getExe pkgs.jq} -r '.configPath' '${hookStateFile}')
          if [ -n "$prevConfigPath" ] && [ "$prevConfigPath" != "null" ]; then
            configFile="${config.devenv.root}/$prevConfigPath"
          fi
        fi

        # Fall back to the current config path if state file doesn't exist or doesn't contain a path
        if [ -z "$configFile" ]; then
          configFile='${config.devenv.root}/${cfg.configPath}'
        fi

        # Only remove if it's a symlink to the nix store
        if $(nix-store --quiet --verify-path "$configFile" > /dev/null 2>&1); then
          echo "Removing $configFile"
          rm "$configFile" || echo "Warning: Failed to uninstall git-hooks at $configFile" >&2
        fi
      '';
    })

    (lib.mkIf anyEnabled {
      ci = [ cfg.run ];
      # Add the packages for any enabled hooks at the end to avoid overriding the language-defined packages.
      packages = lib.mkAfter ([ packageBin ] ++ (cfg.enabledPackages or [ ]));
      tasks = {
        # TODO: split installation script into status + exec
        "devenv:git-hooks:install" = {
          exec = ''
            # Store the current `configPath` in the state file.
            # This is used to remove previous configs when the git-hooks integration is disabled.
            mkdir -p '${hookStateDir}'
            echo "${builtins.toJSON { configPath = cfg.configPath; }}" > '${hookStateFile}'

            # Install the hooks
            ${cfg.installationScript}
          '';
          before = [ "devenv:enterShell" ];
        };
        "devenv:git-hooks:run" = {
          exec = "pre-commit-bin run -a";
          before = [ "devenv:enterTest" ];
        };
      };
    })
  ];
}
