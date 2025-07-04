{ pkgs, config, lib, ... }:

let
  cfg = config.languages.sway;

  sway-overlay = config.lib.getInput {
    name = "sway-overlay";
    url = "github:FuelLabs/fuel.nix";
    attribute = "languages.sway.input";
    follows = [ "nixpkgs" ];
  };
  {
    options.languages.sway = {
      enable = lib.mkEnableOption "tools for Sway development";

      components = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ "forc" "fuel-core" ];
        defaultText = lib.literalExpression ''[ "forc" "fuel-core" ]'';
        description = ''
        Blockchain domain-specific lanugage developed by the [Fuel](https://fuel.network) team as a Rust-based alternative to Solidity.
      '';
      };
    }
  }
