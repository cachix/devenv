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
      type = lib.types.nullOr lib.types.package;
      default =
        if lib.meta.availableOn pkgs.stdenv.hostPlatform pkgs.gdb
        then pkgs.gdb
        else null;
      defaultText = lib.literalExpression "pkgs.gdb";
      description = ''
        An optional debugger package to use with odin.
        The default is `gdb`, if supported on the current system.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      nasm
      clang
      gnumake
      cfg.package
    ] ++ lib.optional (cfg.debugger != null) cfg.debugger
    ++ lib.optional (lib.meta.availableOn pkgs.stdenv.hostPlatform pkgs.ols) pkgs.ols;
  };
}
