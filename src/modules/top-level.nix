{ config, pkgs, lib, ... }:
let
  types = lib.types;
  mkNakedShell = pkgs.callPackage ./mkNakedShell.nix { };
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
    ./mongodb.nix
    ./pre-commit.nix
    ./info.nix
    ./scripts.nix
    ./processes.nix
    ./update-check.nix
  ] ++ map (name: ./. + "/languages/${name}") (builtins.attrNames (builtins.readDir ./languages));

  config = {

    # TODO: figure out how to get relative path without impure mode
    env.DEVENV_ROOT = builtins.getEnv "PWD";
    env.DEVENV_DOTFILE = config.env.DEVENV_ROOT + "/.devenv";
    env.DEVENV_STATE = config.env.DEVENV_DOTFILE + "/state";

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

      shell = mkNakedShell {
        name = "devenv-shell";
        env = config.env;
        profile = pkgs.buildEnv {
          name = "devenv-profile";
          paths = config.packages;
        };
        shellHook = config.enterShell;
      };

      ci = [ config.shell.inputDerivation config.procfile ];
    };
}
