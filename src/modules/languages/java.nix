{ pkgs, config, lib, ... }:

let
  cfg = config.languages.java;
  inherit (lib) types mkEnableOption mkOption mkDefault mkIf optional;
in
{
  options.languages.java = {
    enable = mkEnableOption "Enable tools for Java development.";
  };

  config = mkIf cfg.enable {
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
