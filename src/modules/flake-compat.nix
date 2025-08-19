{ config
, pkgs
, lib
, ...
}:

let
  version = lib.fileContents ./latest-version;
  shellName = config._module.args.name or "default";

  nixFlags = "--show-trace --extra-experimental-features nix-command --extra-experimental-features flakes";

  # Helper function to wrap commands with nix develop
  #
  # This is skipped if the user is already in a shell launched by direnv.
  # We trust that direnv will handle reloads.
  wrapWithNixDevelop = command: args: ''
    if [[ -n "$IN_NIX_SHELL" && "$DEVENV_IN_DIRENV_SHELL" == "true" ]]; then
      exec ${command} ${args}
    else
      exec nix develop .#${shellName} --impure ${nixFlags} -c ${command} ${args}
    fi
  '';

  # Flake integration wrapper for devenv CLI
  devenvFlakeWrapper = pkgs.writeScriptBin "devenv" ''
    #!/usr/bin/env bash

    # we want subshells to fail the program
    set -e

    command=$1
    if [[ ! -z $command ]]; then
      shift
    fi

    case $command in
      up)
        # Re-enter the shell to ensure we use the latest configuration
        ${wrapWithNixDevelop "devenv-flake-up" "\"$@\""}
        ;;

      test)
        # Re-enter the shell to ensure we use the latest configuration
        ${wrapWithNixDevelop "devenv-flake-test" "\"$@\""}
        ;;

      version)
        echo "devenv: ${version}"
        ;;

      *)
        echo "https://devenv.sh (version ${version}): Fast, Declarative, Reproducible, and Composable Developer Environments"
        echo
        echo "This is a flake integration wrapper that comes with a subset of functionality from the flakeless devenv CLI."
        echo
        echo "Usage: devenv command"
        echo
        echo "Commands:"
        echo
        echo "test            Runs tests"
        echo "up              Starts processes in foreground. See http://devenv.sh/processes"
        echo "version         Display devenv version"
        echo
        exit 1
    esac
  '';
in
{
  config = lib.mkIf config.devenv.flakesIntegration {
    env.DEVENV_FLAKE_SHELL = shellName;

    packages = [
      # Flake integration wrapper
      devenvFlakeWrapper

      # Add devenv-flake-up and devenv-flake-test scripts
      (pkgs.writeShellScriptBin "devenv-flake-up" ''
        ${lib.optionalString (config.processes == { }) ''
          echo "No 'processes' option defined: https://devenv.sh/processes/" >&2
          exit 1
        ''}
        exec ${config.procfileScript} "$@"
      '')

      (pkgs.writeShellScriptBin "devenv-flake-test" ''
        exec ${config.test} "$@"
      '')
    ];
  };
}
