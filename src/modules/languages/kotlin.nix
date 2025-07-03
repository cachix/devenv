{ pkgs, config, lib, ... }:

let
  cfg = config.languages.kotlin;
in
{
  options.languages.kotlin = {
    enable = lib.mkEnableOption "tools for Kotlin development";

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Kotlin development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Kotlin language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.kotlin-language-server;
          defaultText = lib.literalExpression "pkgs.kotlin-language-server";
          description = "The kotlin-language-server package to use.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable ktlint formatter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.ktlint;
          defaultText = lib.literalExpression "pkgs.ktlint";
          description = "The ktlint package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      kotlin
      gradle
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
        lib.optional cfg.dev.formatter.enable cfg.dev.formatter.package
    );
  };
}
