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
  defaultModules = [ 
    ./postgres.nix 
    ./pre-commit.nix
  ];
in {
  options = {
    env = lib.mkOption {
      type = types.attrs;
      description = "TODO";
      default = {};
    };

    includes = lib.mkOption {
      type = types.listOf types.path;
      description = "TODO";
      default = [];
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

    shell = lib.mkOption {
      type = types.package;
      internal = true;
    };

    build = lib.mkOption {
      type = types.package;
      internal = true;
    };

    yamls = lib.mkOption {
      type = types.listOf types.path;
      internal = true;
    };

    nixes = lib.mkOption {
      type = types.listOf types.path;
      internal = true;
    };
  };

  imports = defaultModules;

  config = {
    
    nixes = map (path: path + "/devenv.nix" ) config.includes;

    yamls = map (path: path + "/devenv.yml" ) config.includes;

    procfile = pkgs.writeText "procfile" (lib.mapAttrs (name: process: "${name}: ${process.cmd}") config.processes);

    shell = pkgs.mkShell ({
      name = "devenv";
      packages = config.packages;
      shellHook = config.enterShell;
    } // config.env);

    build = pkgs.symlinkJoin { 
      name = "devenv-gc"; 
      paths = [ config.shell config.procfile ];
    };    
  };
}