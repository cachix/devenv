{ pkgs, config, lib, ... }:

let
  cfg = config.languages.vala;
in
{
  options.languages.vala = {
    enable = lib.mkEnableOption "tools for Vala development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.vala;
      defaultText = lib.literalExpression "pkgs.vala";
      description = "The Vala package to use.";
      example = lib.literalExpression "pkgs.vala_0_54";
    };

    lsp = {
      enable = lib.mkEnableOption "Vala Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.vala-language-server;
        defaultText = lib.literalExpression "pkgs.vala-language-server";
        description = "The Vala language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
