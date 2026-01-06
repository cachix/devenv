{ pkgs, config, lib, ... }:

let
  cfg = config.languages.nim;
in
{
  options.languages.nim = {
    enable = lib.mkEnableOption "tools for Nim development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.nim;
      defaultText = lib.literalExpression "pkgs.nim";
      description = "The Nim package to use.";
    };

    lsp = {
      enable = lib.mkEnableOption "Nim Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.nimlangserver;
        defaultText = lib.literalExpression "pkgs.nimlangserver";
        description = "The Nim language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
