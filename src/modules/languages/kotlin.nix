{ pkgs, config, lib, ... }:

let
  cfg = config.languages.kotlin;
in
{
  options.languages.kotlin = {
    enable = lib.mkEnableOption "tools for Kotlin development";
    lsp = {
      enable = lib.mkEnableOption "Kotlin Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.kotlin-language-server;
        defaultText = lib.literalExpression "pkgs.kotlin-language-server";
        description = "The Kotlin language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      kotlin
      gradle
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
