{ pkgs, config, lib, ... }:

let
  cfg = config.languages.pkl;
in
{
  options.languages.pkl = {
    enable = lib.mkEnableOption "tools for Pkl development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.pkl;
      defaultText = lib.literalExpression "pkgs.pkl";
      description = "The Pkl package to use.";
    };

    lsp = {
      enable = lib.mkEnableOption "Pkl Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.pkl-lsp;
        defaultText = lib.literalExpression "pkgs.pkl-lsp";
        description = "The Pkl language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
