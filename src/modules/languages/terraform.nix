{ pkgs, config, lib, ... }:

let
  cfg = config.languages.terraform;

  nixpkgs-terraform = config.lib.getInput {
    name = "nixpkgs-terraform";
    url = "github:stackbuilders/nixpkgs-terraform";
    attribute = "languages.terraform.version";
  };
in
{
  options.languages.terraform = {
    enable = lib.mkEnableOption "tools for Terraform development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.terraform;
      defaultText = lib.literalExpression "pkgs.terraform";
      description = "The Terraform package to use.";
    };

    version = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        The Terraform version to use.
        This automatically sets the `languages.terraform.package` using [nixpkgs-terraform](https://github.com/stackbuilders/nixpkgs-terraform).
      '';
      example = "1.5.0 or 1.6.2";
    };
  };

  config = lib.mkIf cfg.enable {
    languages.terraform.package = lib.mkMerge [
      (lib.mkIf (cfg.version != null) (nixpkgs-terraform.packages.${pkgs.stdenv.system}.${cfg.version} or (throw "Unsupported Terraform version, update the nixpkgs-terraform input or go to https://github.com/stackbuilders/nixpkgs-terraform/blob/main/versions.json for the full list of supported versions.")))
    ];

    packages = with pkgs; [
      cfg.package
      terraform-ls
      tfsec
    ];
  };
}
