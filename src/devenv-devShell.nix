{ config, pkgs }:
let
  lib = pkgs.lib;
  version = lib.fileContents ./modules/latest-version;
  shellPrefix = shellName: if shellName == "default" then "" else "${shellName}-";
in
pkgs.writeScriptBin "devenv" ''
  #!/usr/bin/env bash

  # we want subshells to fail the program
  set -e

  NIX_FLAGS="--show-trace --extra-experimental-features nix-command --extra-experimental-features flakes"

  command=$1
  if [[ ! -z $command ]]; then
    shift
  fi

  case $command in
    up)
      procfilescript=$(nix build '.#${shellPrefix (config._module.args.name or "default")}devenv-up' --no-link --print-out-paths --impure)
      if [ "$(cat $procfilescript|tail -n +2)" = "" ]; then
        echo "No 'processes' option defined: https://devenv.sh/processes/"
        exit 1
      else
        exec $procfilescript "$@"
      fi
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
      echo "up              Starts processes in foreground. See http://devenv.sh/processes"
      echo "version         Display devenv version"
      echo
      exit 1
  esac
''
