{ pkgs, config, lib, ... }:

let
  cfg = config.languages.odin;
in
{
  options.languages.odin = {
    enable = lib.mkEnableOption "tools for Odin Language";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.odin;
      defaultText = lib.literalExpression "pkgs.odin";
      description = "The odin package to use.";
    };

    debugger = lib.mkOption {
      type = lib.types.package;
      default = pkgs.gdb;
      defaultText = lib.literalExpression "pkgs.gdb";
      description = "The debugger package to use with odin.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      gnumake
      ols
      cfg.debugger
      cfg.package
    ];
  };
}
