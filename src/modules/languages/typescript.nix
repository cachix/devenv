{ pkgs, config, lib, ... }:

let
  cfg = config.languages.typescript;
in
{
  options.languages.typescript = {
    enable = lib.mkEnableOption "tools for TypeScript development";

    lsp = {
      enable = lib.mkEnableOption "TypeScript Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.typescript-language-server;
        defaultText = lib.literalExpression "pkgs.typescript-language-server";
        description = "The TypeScript language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      pkgs.typescript
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
