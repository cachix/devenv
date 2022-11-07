{ pkgs, config, lib, ... }:

let
  cfg = config.languages.javascript;
in
{
  options.languages.javascript = {
    enable = lib.mkEnableOption "Enable tools for JavaScript development.";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      nodejs
    ];

    enterShell = ''
      echo node --version
      node --version
    '';
  };
}
