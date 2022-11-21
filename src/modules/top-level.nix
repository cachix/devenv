{ config, pkgs, lib, ... }:

let
  types = lib.types;
  processType = types.submodule ({ config, ... }: {
    options = {
      exec = lib.mkOption {
        type = types.str;
        description = "Bash code to run the process.";
      };
    };
  });
in
{
  options = {
    env = lib.mkOption {
      type = types.attrs;
      description = "Environment variables to be exposed inside the developer environment.";
      default = { };
    };

    enterShell = lib.mkOption {
      type = types.lines;
      description = "Bash code to execute when entering the shell.";
      default = "";
    };

    packages = lib.mkOption {
      type = types.listOf types.package;
      description = "A list of packages to expose inside the developer environment. Search available packages using ``devenv search NAME``.";
      default = [ ];
    };

    processes = lib.mkOption {
      type = types.attrsOf processType;
      default = { };
      description = "Processes can be started with ``devenv up`` and run in foreground mode.";
    };

    process.implementation = lib.mkOption {
      type = types.enum [ "honcho" "overmind" ];
      description = "The implimentation used when performing ``devenv up``.";
      default = "honcho";
      example = "overmind";
    };

    # INTERNAL

    procfile = lib.mkOption {
      type = types.package;
      internal = true;
    };

    procfileEnv = lib.mkOption {
      type = types.package;
      internal = true;
    };

    procfileScript = lib.mkOption {
      type = types.package;
      internal = true;
    };

    shell = lib.mkOption {
      type = types.package;
      internal = true;
    };

    ci = lib.mkOption {
      type = types.listOf types.package;
      internal = true;
    };
  };

  imports = [
    ./postgres.nix
    ./redis.nix
    ./mysql.nix
    ./pre-commit.nix
    ./scripts.nix
    ./update-check.nix
  ] ++ map (name: ./. + "/languages/${name}") (builtins.attrNames (builtins.readDir ./languages));

  config = {
    # TODO: figure out how to get relative path without impure mode
    env.DEVENV_ROOT = builtins.getEnv "PWD";
    env.DEVENV_DOTFILE = config.env.DEVENV_ROOT + "/.devenv";
    env.DEVENV_STATE = config.env.DEVENV_DOTFILE + "/state";

    procfile = pkgs.writeText "procfile"
      (lib.concatStringsSep "\n" (lib.mapAttrsToList (name: process: "${name}: ${process.exec}") config.processes));

    procfileEnv = pkgs.writeText "procfile-env"
      (lib.concatStringsSep "\n" (lib.mapAttrsToList (name: value: "${name}=${toString value}") config.env));

    procfileScript = {
      honcho = pkgs.writeShellScript "honcho-up" ''
        echo "Starting processes ..." 1>&2
        echo "" 1>&2
        ${pkgs.honcho}/bin/honcho start -f ${config.procfile} --env ${config.procfileEnv}
      '';
      overmind = pkgs.writeShellScript "overmind-up" ''
        OVERMIND_ENV=${config.procfileEnv} ${pkgs.overmind}/bin/overmind start --procfile ${config.procfile}
      '';
    }.${config.process.implementation};

    enterShell = ''
      export PS1="(devenv) $PS1"
      
      # note what environments are active, but make sure we don't repeat them
      if [[ ! "$DIRENV_ACTIVE" =~ (^|:)"$PWD"(:|$) ]]; then
        export DIRENV_ACTIVE="$PWD:$DIRENV_ACTIVE"
      fi

      # devenv helper
      if [ ! type -p direnv &>/dev/null && -f .envrc ]; then
        echo "You have .envrc but direnv command is not installed."
        echo "Please install direnv: https://direnv.net/docs/installation.html"
      fi
    '';

    shell = pkgs.mkShell ({
      name = "devenv";
      packages = config.packages;
      shellHook = config.enterShell;
    } // config.env);

    ci = [ config.shell config.procfile ];
  };
}
