{ config, lib, ... }:

let
  testType = lib.types.submodule ({ config, ... }: {
    options = {
      tags = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ "local" ];
        description = "Tags for this test.";
      };

      nix = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        example = "{ pkgs, ... }: {}";
        description = "devenv.nix code.";
      };

      yaml = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        example = ''
          inputs:
            nixpkgs:
              url: github:NixOS/nixpkgs/nixpkgs-unstable
        '';
        description = "devenv.yaml code.";
      };

      test = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        description = "Bash to be executed.";
        default = null;
      };

      src = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = "Source code with all the files.";
      };
    };
  });
in
{
  options.tests = lib.mkOption {
    type = lib.types.attrsOf testType;
    default = { };
    description = "Tests for this module.";
  };

  config.assertions =
    let
      mk = name: cfg:
        {
          assertion = cfg.nix != null || cfg.src != null;
          message = "Either tests.${name}.nix or tests.${name}.src needs to be defined.";
        };
    in
    lib.mapAttrsToList mk config.tests;
}
