{ pkgs, config, lib, ... }:

let
  cfg = config.languages.lua;
in
{
  options.languages.lua = {
    enable = lib.mkEnableOption "tools for Lua development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.lua;
      defaultText = lib.literalExpression "pkgs.lua";
      description = "The Lua package to use.";
    };

    lsp = {
      enable = lib.mkEnableOption "Lua Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.lua-language-server;
        defaultText = lib.literalExpression "pkgs.lua-language-server";
        description = "The Lua language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
