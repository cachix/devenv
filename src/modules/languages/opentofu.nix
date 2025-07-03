{ pkgs, config, lib, ... }:

let
  cfg = config.languages.opentofu;
in
{
  options.languages.opentofu = {
    enable = lib.mkEnableOption "tools for OpenTofu development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.opentofu;
      defaultText = lib.literalExpression "pkgs.opentofu";
      description = "The OpenTofu package to use.";
    };

    lsp = {
      enable = lib.mkEnableOption "Terraform/OpenTofu language server";

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.terraform-lsp;
        defaultText = lib.literalExpression "pkgs.terraform-lsp";
        description = "Terraform/OpenTofu language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.package
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
