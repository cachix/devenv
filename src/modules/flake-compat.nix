{
  config,
  pkgs,
  lib,
  ...
}:

let
  version = lib.fileContents ./latest-version;
  shellName = config._module.args.name or "default";
  nixFlags = "--show-trace --extra-experimental-features nix-command --extra-experimental-features flakes";

  # Helper function to wrap commands with nix develop
  wrapWithNixDevelop =
    command: args: "exec nix develop .#${shellName} --impure ${nixFlags} -c ${command} ${args}";

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
      container)
        subcommand=$1
        if [[ ! -z $subcommand ]]; then
          shift
        fi

        case $subcommand in
          build|copy|run)
            # Re-enter the shell to ensure we use the latest configuration
            ${wrapWithNixDevelop "devenv-flake-container-$subcommand" "\"$@\""}
            ;;
          *)
            echo "Usage: devenv container <subcommand> <name> [args...]"
            echo
            echo "Subcommands:"
            echo "  build <name>    Build a container"
            echo "  copy <name>     Copy a container to a registry"
            echo "  run <name>      Run a container"
            echo
            exit 1
            ;;
        esac
        ;;
      up)
        # Re-enter the shell to ensure we use the latest configuration
        ${wrapWithNixDevelop "devenv-flake-up" "\"$@\""}
        ;;

      test)
        # Re-enter the shell to ensure we use the latest configuration
        ${wrapWithNixDevelop "devenv-flake-test" "\"$@\""}
        ;;

      version)
        ${wrapWithNixDevelop "echo" "\"devenv: ${version}\""}
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
        echo "container       Build, copy and run containers. See https://devenv.sh/containers"
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

    packages =
      [
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

        # Container subcommand scripts
        (pkgs.writeShellScriptBin "devenv-flake-container-build" ''
          if [[ "$(uname)" == "Darwin" ]]; then
            echo "Containers are not supported on macOS yet: https://github.com/cachix/devenv/issues/430" >&2
            exit 1
          fi

          name=$1
          if [[ -z $name ]]; then
            echo "Usage: devenv container build <name>" >&2
            exit 1
          fi
          shift

          case "$name" in
            ${lib.concatMapStringsSep "|" (name: lib.escapeShellArg name) (lib.attrNames config.containers)})
              exec devenv-flake-container-build-$name "$@"
              ;;
            *)
              echo "Container '$name' not found. Available containers:" >&2
              echo "  ${lib.concatStringsSep "\n  " (lib.attrNames config.containers)}" >&2
              exit 1
              ;;
          esac
        '')

        (pkgs.writeShellScriptBin "devenv-flake-container-copy" ''
          name=$1
          if [[ -z $name ]]; then
            echo "Usage: devenv container copy <name> [registry] [args...]" >&2
            exit 1
          fi
          shift

          case "$name" in
            ${lib.concatMapStringsSep "|" (name: lib.escapeShellArg name) (lib.attrNames config.containers)})
              exec devenv-flake-container-copy-$name "$@"
              ;;
            *)
              echo "Container '$name' not found. Available containers:" >&2
              echo "  ${lib.concatStringsSep "\n  " (lib.attrNames config.containers)}" >&2
              exit 1
              ;;
          esac
        '')

        (pkgs.writeShellScriptBin "devenv-flake-container-run" ''
          name=$1
          if [[ -z $name ]]; then
            echo "Usage: devenv container run <name> [args...]" >&2
            exit 1
          fi
          shift

          # Warning if registry is provided
          if [[ "$1" == "--registry" ]] || [[ "$1" =~ ^[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}(:[0-9]+)?$ ]]; then
            echo "Warning: Ignoring --registry flag when running container" >&2
            shift
          fi

          case "$name" in
            ${lib.concatMapStringsSep "|" (name: lib.escapeShellArg name) (lib.attrNames config.containers)})
              exec devenv-flake-container-run-$name "$@"
              ;;
            *)
              echo "Container '$name' not found. Available containers:" >&2
              echo "  ${lib.concatStringsSep "\n  " (lib.attrNames config.containers)}" >&2
              exit 1
              ;;
          esac
        '')
      ]
      # Individual container command scripts
      ++ (lib.flatten (
        lib.mapAttrsToList (containerName: container: [
          # Build script for each container
          (pkgs.writeShellScriptBin "devenv-flake-container-build-${containerName}" ''
            echo "Building container '${containerName}'..."
            container_path=${container.derivation}
            echo "$container_path"
          '')

          # Copy script for each container
          (pkgs.writeShellScriptBin "devenv-flake-container-copy-${containerName}" ''
            # Prepare registry argument
            registry="$1"
            if [[ -n "$registry" ]]; then
              shift
            else
              registry="false"
            fi

            # Build the container first
            echo "Building container '${containerName}'..."
            container_path=${container.derivation}

            # Run the copy script
            echo "Copying container..."
            exec ${container.copyScript} "$container_path" "$registry" "$@"
          '')

          # Run script for each container
          (pkgs.writeShellScriptBin "devenv-flake-container-run-${containerName}" ''
            # Build the container first
            echo "Building container '${containerName}'..."
            container_path=${container.derivation}

            # Copy to docker-daemon
            echo "Copying container..."
            ${container.copyScript} "$container_path" "docker-daemon:" || exit 1

            # Run the container
            echo "Running container '${containerName}'..."
            exec ${container.dockerRun} "$@"
          '')
        ]) config.containers
      ));
  };
}
