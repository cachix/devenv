{ pkgs, config, lib, ... }:

let
  cfg = config.languages.c;
in
{
  options.languages.c = {
    enable = lib.mkEnableOption "tools for C development";

    debugger = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default =
        if !(pkgs.stdenv.isAarch64 && pkgs.stdenv.isLinux) && lib.meta.availableOn pkgs.stdenv.hostPlatform pkgs.gdb
        then pkgs.gdb
        else null;
      defaultText = lib.literalExpression "pkgs.gdb";
      description = ''
        An optional debugger package to use with c.
        The default is `gdb`, if supported on the current system.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      stdenv
      gnumake
      ccls
      pkg-config
    ] ++ lib.optional (cfg.debugger != null) cfg.debugger
    ++ lib.optional (lib.meta.availableOn pkgs.stdenv.hostPlatform pkgs.valgrind && !pkgs.valgrind.meta.broken) pkgs.valgrind;
  };
}
