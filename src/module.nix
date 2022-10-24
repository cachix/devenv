{ config, pkgs, ... }:

let
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

    includes = lib.mkOption {
      type = types.listOf types.path;
      description = "TODO";
      default = [];
    };

    enterShell = lib.mkOption {
      type = types.lines;
      description = "TODO";
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
      type = types.str;
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

  config = {
    procfile = lib.mapAttrs (name: process: "${name}": ${process.cmd}) config.processes;

    shell = pkgs.mkShell {
      name = "devenv";
      packages = config.packages;
      shellHook = config.enterShell;
    };

    build = symlinkJoin { 
      name = "devenv-all"; 
      paths = [ config.shell config.procfile ];
    };
  };
}