{ pkgs, config, lib, ... }:

let
  cfg = config.languages.scala;
in
{
  options.languages.scala = {
    enable = lib.mkEnableOption "Enable tools for Scala development.";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      scala
      sbt
    ];

    enterShell = ''
      scala --version

      sbt --version
    '';
  };
}
