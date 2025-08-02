{ config, lib, pkgs, ... }:

let
  types = lib.types;
in
{
  options = {
    git = {
      root = lib.mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "The root directory of the Git repository. Automatically set to the output of 'git rev-parse --show-toplevel' if available.";
      };
    };
  };
}
