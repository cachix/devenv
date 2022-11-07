{ config, pkgs, lib, ... }:

let
  types = lib.types;
  processType = types.submodule ({ config, ... }: {
    options = {
      exec = lib.mkOption {
        type = types.str;
        description = "TODO";
      };
    };
  });
in
{
  options = {
    env = lib.mkOption {
      type = types.attrs;
      description = "TODO";
      default = { };
    };

    enterShell = lib.mkOption {
      type = types.lines;
      description = "TODO";
      default = "";
    };

    packages = lib.mkOption {
      type = types.listOf types.package;
      description = "TODO";
      default = [ ];
    };

    processes = lib.mkOption {
      type = types.attrsOf processType;
      default = { };
      description = "TODO";
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
    ./pre-commit.nix
    ./scripts.nix
  ] ++ map (name: ./. + "/languages/${name}") (builtins.attrNames (builtins.readDir ./languages));

  config = {
    env.DEVENV_DOTFILE = ".devenv/";
    env.DEVENV_STATE = config.env.DEVENV_DOTFILE + "state/";

    procfile = pkgs.writeText "procfile"
      (lib.concatStringsSep "\n" (lib.mapAttrsToList (name: process: "${name}: ${process.exec}") config.processes));

    procfileEnv = pkgs.writeText "procfile-env"
      (lib.concatStringsSep "\n" (lib.mapAttrsToList (name: value: "${name}=${toString value}") config.env));

    enterShell = ''
      export PS1="(direnv) $PS1"
      
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
