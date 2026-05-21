# Compatibility layer for devenv to work inside of a `nix develop` shell.
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
  # This is skipped if the user is already in a devenv shell loaded by direnv.
  # direnv watches flake.nix/flake.lock and reloads the env automatically, so
  # re-entering would be wasted work. Outside direnv (raw `nix develop`, fresh
  # invocation, foreign-flake consumer), we re-enter to pick up any changes;
  # nix resolves `.` by walking up for flake.nix and surfaces its own error if
  # none is found.
  #
  # `$DIRENV_DIR` is exported by direnv on every load. `$DEVENV_IN_DIRENV_SHELL`
  # is the legacy opt-in flag from the official templates; honored for
  # backwards compatibility with older `.envrc` files.
  wrapWithNixDevelop = command: args: ''
    if [[ -n "$DEVENV_PROFILE" && ( -n "$DIRENV_DIR" || "$DEVENV_IN_DIRENV_SHELL" == "true" ) ]]; then
      exec ${command} ${args}
    else
      exec nix develop .#${shellName} --impure ${nixFlags} -c ${command} ${args}
    fi
  '';

  # Flake integration wrapper for devenv CLI
  devenv-flake-wrapper = pkgs.writeScriptBin "devenv" ''
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

      tasks)
        # Re-enter the shell to ensure we use the latest configuration
        ${wrapWithNixDevelop "devenv-flake-tasks" "\"$@\""}
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
        echo "tasks           Manage and run tasks"
        echo "test            Run tests"
        echo "up              Start processes in the foreground. See http://devenv.sh/processes"
        echo "version         Display the devenv version"
        echo
        exit 1
    esac
  '';

  # `devenv up` helper command
  devenv-flake-up =
    pkgs.writeShellScriptBin "devenv-flake-up" ''
      ${lib.optionalString (config.processes == { }) ''
        echo "No 'processes' option defined: https://devenv.sh/processes/" >&2
        exit 1
      ''}
      exec ${config.procfileScript} "$@"
    '';

  # `devenv test` helper command
  devenv-flake-test =
    pkgs.writeShellScriptBin "devenv-flake-test" ''
      echo "• Testing ..." >&2
      exec ${config.test} "$@"
    '';

  # `devenv tasks` helper command
  devenv-flake-tasks =
    pkgs.writeShellScriptBin "devenv-flake-tasks" ''
      subcommand=$1
      shift
      case "$subcommand" in
        run)
          exec ${config.task.package}/bin/devenv-tasks run \
            --cache-dir ${lib.escapeShellArg config.devenv.dotfile} \
            --runtime-dir ${lib.escapeShellArg config.devenv.runtime} \
            "$@"
          ;;
        *)
          exec ${config.task.package}/bin/devenv-tasks "$subcommand" "$@"
          ;;
      esac
    '';

  devenvFlakeCompat = pkgs.symlinkJoin {
    name = "devenv-flake-compat";
    paths = [
      devenv-flake-wrapper
      devenv-flake-up
      devenv-flake-test
      devenv-flake-tasks
    ];
  };
in
{
  config = lib.mkIf config.devenv.flakesIntegration {
    assertions = [
      {
        assertion = config.devenv.root != "";
        message = ''
          devenv was not able to determine the current directory.

          See https://devenv.sh/guides/using-with-flakes/ how to use it with flakes.
        '';
      }
    ];

    devenv.root = lib.mkDefault (builtins.getEnv "PWD");
    # Used for TMPDIR override - should NOT use XDG_RUNTIME_DIR as that's
    # a small tmpfs meant for runtime files (sockets), not build artifacts
    devenv.tmpdir =
      let
        tmp = builtins.getEnv "TMPDIR";
      in
      lib.mkDefault (if tmp != "" then tmp else "/tmp");

    env.DEVENV_FLAKE_SHELL = shellName;

    # Add the flake command helpers directly to the path.
    # This is to avoid accidentally adding their paths to env vars, like DEVENV_PROFILE.
    # If that happens and a profile command is provided the full env, we will create a recursive dependency between the env and the procfile command.
    enterShell = ''
      export PATH=${devenvFlakeCompat}/bin:$PATH
    '';
  };
}
