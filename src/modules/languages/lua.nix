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

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Lua development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Lua language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.lua-language-server;
          defaultText = lib.literalExpression "pkgs.lua-language-server";
          description = ''
            The lua-language-server package to use.
            
            lua-language-server (LuaLS) is the most popular and actively maintained LSP for Lua.
            
            Alternative LSPs available in nixpkgs:
            - lua-lsp: Earlier implementation by Alloyed
            - EmmyLua-LanguageServer: IntelliJ-based LSP (not available in nixpkgs)
          '';
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Lua formatter (stylua).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.stylua;
          defaultText = lib.literalExpression "pkgs.stylua";
          description = "The stylua package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional (cfg.dev.lsp.enable) cfg.dev.lsp.package ++
        lib.optional (cfg.dev.formatter.enable) cfg.dev.formatter.package
    );
  };
}
