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
      enable = lib.mkEnableOption "OpenTofu Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.terraform-ls;
        defaultText = lib.literalExpression "pkgs.terraform-ls";
        description = "The OpenTofu language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    git-hooks.hooks = {
      terraform-format.package = config.lib.mkOverrideDefault cfg.package;
      terraform-validate.package = config.lib.mkOverrideDefault cfg.package;
    };

    packages = [
      cfg.package
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
