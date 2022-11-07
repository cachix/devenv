{ pkgs, config, lib, ... }:

let
  cfg = config.languages.php;
in
{
  options.languages.php = {
    enable = lib.mkEnableOption "Enable tools for OHP development.";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      php
    ];

    enterShell = ''
      php --version
    '';
  };
}
