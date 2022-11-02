{ pkgs, lib, config, ... }:

let
  types = lib.types;
  scriptType = types.submodule ({ config, ... }: {
    options = {
      exec = lib.mkOption {
        type = types.str;
        description = "TODO";
      };
    };
  });
  toPackage = name: script: pkgs.writeShellScriptBin name ''
    #!${pkgs.bash}/bin/bash

    ${script.exec}
  '';
in {
  options = {
    scripts = lib.mkOption {
      type = types.attrsOf scriptType;
      default = {};
      description = "TODO";
    };
  };

  config = {
    packages = lib.mapAttrsToList toPackage config.scripts;
  };
}