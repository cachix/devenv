{ config, pkgs, lib, ... }:
let
  types = lib.types;
  # Returns a list of all the entries in a folder
  listEntries = path:
    map (name: path + "/${name}") (builtins.attrNames (builtins.readDir path));
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
    ./devcontainer.nix
    ./info.nix
    ./pre-commit.nix
    ./processes.nix
    ./scripts.nix
    ./update-check.nix
  ]
  ++ (listEntries ./languages)
  ++ (listEntries ./services)
  ;

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

    shell = pkgs.mkShell ({
      name = "devenv";
      packages = config.packages;
      shellHook = config.enterShell;
    } // config.env);

    ci = [ config.shell config.procfile ];
  };
}
