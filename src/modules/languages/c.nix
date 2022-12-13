{ pkgs, config, lib, ... }:

let
  cfg = config.languages.c;
in
{
  options.languages.c = {
    enable = lib.mkEnableOption "Enable tools for C development.";
  };


  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      gcc
    ];
  };
}
