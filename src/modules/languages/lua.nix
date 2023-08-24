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
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.lua-language-server
    ];
  };
}
