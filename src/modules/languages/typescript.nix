{ pkgs, config, lib, ... }:

let
  cfg = config.languages.typescript;
in
{
  options.languages.typescript = {
    enable = lib.mkEnableOption "tools for TypeScript development";

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable TypeScript development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable typescript-language-server language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.nodePackages.typescript-language-server;
          defaultText = lib.literalExpression "pkgs.nodePackages.typescript-language-server";
          description = "The typescript-language-server package to use. This wraps Microsoft's tsserver and provides LSP support for both JavaScript and TypeScript.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable prettier formatter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.prettier;
          defaultText = lib.literalExpression "pkgs.prettier";
          description = "The prettier package to use.";
        };
      };

      linter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable eslint linter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.eslint;
          defaultText = lib.literalExpression "pkgs.eslint";
          description = "The eslint package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      pkgs.typescript
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
        lib.optional cfg.dev.formatter.enable cfg.dev.formatter.package ++
        lib.optional cfg.dev.linter.enable cfg.dev.linter.package
    );
  };
}
