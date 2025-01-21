{ pkgs, config, lib, ... }:

let
  cfg = config.languages.solidity;

  foundry = config.lib.getInput {
    name = "foundry";
    url = "github:shazow/foundry.nix";
    attribute = "languages.solidity.foundry.package";
    follows = [ "nixpkgs" ];
  };
in
{
  options.languages.solidity = {
    enable = lib.mkEnableOption "tools for Solidity development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which compiler of Solidity to use.";
      default = pkgs.solc;
      defaultText = lib.literalExpression "pkgs.solc";
    };

    foundry = {
      enable = lib.mkEnableOption "install Foundry";

      package = lib.mkOption {
        type = lib.types.package;
        description = "Which Foundry package to use.";
        default = foundry.defaultPackage.${pkgs.stdenv.system};
        defaultText = lib.literalExpression "foundry.defaultPackage.$${pkgs.stdenv.system}";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ] ++ lib.optional cfg.foundry.enable (cfg.foundry.package);
  };
}
