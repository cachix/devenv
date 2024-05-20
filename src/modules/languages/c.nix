{ pkgs, config, lib, ... }:

let
  cfg = config.languages.c;
in
{
  options.languages.c = {
    enable = lib.mkEnableOption "tools for C development";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      valgrind
      gdb
      stdenv
      gnumake
      ccls
      pkg-config
    ];
  };
}
