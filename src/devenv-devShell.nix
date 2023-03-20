{ config, pkgs }:
let
  lib = pkgs.lib;

  app = pkgs.writeShellApplication {
    name = "devenv-flake-cli";
    runtimeInputs = with pkgs; [ docopts ];
    text = builtins.readFile ./devenv-devShell.sh;
  };

  envs = lib.concatStringsSep " " (lib.mapAttrsToList lib.toShellVar {
    PROCFILESCRIPT = config.procfileScript;
    VERSION = lib.fileContents ./modules/latest-version;
    CUSTOM_NIX_BIN = "${pkgs.nix}/bin";
  });
in
pkgs.writeScriptBin "devenv" ''${envs} "${app}/bin/devenv-flake-cli" "$@"''
