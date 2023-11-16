{ pkgs, config, lib, inputs, ... }:

let
  cfg = config.languages.terraform;

  nixpkgs-terraform = inputs.nixpkgs-terraform or (throw ''
    To use languages.terraform.version, you need to add the following to your devenv.yaml:

      inputs:
        nixpkgs-terraform:
          url: github:stackbuilders/nixpkgs-terraform
  '');
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
      (lib.mkIf (cfg.version != null) (nixpkgs-terraform.packages.${pkgs.stdenv.system}.${cfg.version} or (throw "Unsupported Terraform version, see https://github.com/stackbuilders/nixpkgs-terraform#available-versions")))
    ];

    packages = with pkgs; [
      cfg.package
      terraform-ls
      tfsec
    ];
  };
}
