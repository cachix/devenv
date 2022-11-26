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
      scala-cli
      sbt
      coursier
      scalafmt
    ];

    languages.java.enable = true;

    enterShell = ''
      scala --version
      scala-cli --version
      sbt --version
      scalafmt --version
      echo cs version
      cs version
    '';
  };
}
