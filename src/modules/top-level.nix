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
in {
  options = {
    env = lib.mkOption {
      type = types.attrs;
      description = "TODO";
      default = {};
    };

    enterShell = lib.mkOption {
      type = types.lines;
      description = "TODO";
      default = "";
    };

    packages = lib.mkOption {
      type = types.listOf types.package;
      description = "TODO";
      default = [];
    };

    processes = lib.mkOption {
      type = types.attrsOf processType;
      default = {};
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

    build = lib.mkOption {
      type = types.package;
      internal = true;
    };
  };

  imports = [ 
    ./postgres.nix 
    ./pre-commit.nix
    ./scripts.nix
  ];

  config = {

    procfile = pkgs.writeText "procfile" 
      (lib.concatStringsSep "\n" (lib.mapAttrsToList (name: process: "${name}: ${process.exec}") config.processes));
    
    procfileEnv = pkgs.writeText "procfile-env"
      (lib.concatStringsSep "\n" (lib.mapAttrsToList (name: value: "${name}=${toString value}") config.env));

    enterShell = ''
      export PS1="(direnv) $PS1"
    '';

    shell = pkgs.mkShell ({
      name = "devenv";
      packages = config.packages;
      shellHook = config.enterShell;
    } // config.env);

    build = pkgs.runCommand "devenv-build" {} ''
      ls ${config.shell}
      ls ${config.procfile}
      touch $out
    '';
  };
}