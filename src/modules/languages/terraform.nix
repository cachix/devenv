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

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Terraform development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Terraform language server (terraform-ls).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.terraform-ls;
          defaultText = lib.literalExpression "pkgs.terraform-ls";
          description = "The terraform-ls package to use.";
        };
      };

      linter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable tfsec linter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.tfsec;
          defaultText = lib.literalExpression "pkgs.tfsec";
          description = "The tfsec package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    languages.terraform.package = lib.mkMerge [
      (lib.mkIf (cfg.version != null) (nixpkgs-terraform.packages.${pkgs.stdenv.system}.${cfg.version} or (throw "Unsupported Terraform version, update the nixpkgs-terraform input or go to https://github.com/stackbuilders/nixpkgs-terraform/blob/main/versions.json for the full list of supported versions.")))
    ];

    packages = [
      cfg.package
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional (cfg.dev.lsp.enable) cfg.dev.lsp.package ++
        lib.optional (cfg.dev.linter.enable) cfg.dev.linter.package
    );
  };
}
