{ pkgs, config, lib, ... }:

let
  cfg = config.languages.java;
in
{
  options.languages.java = {
    enable = lib.mkEnableOption "Enable tools for Java development.";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      gradle
      jdk
      maven
    ];

    enterShell = ''
      mvn -version
    '';
  };
}
