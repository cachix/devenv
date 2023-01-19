{ pkgs, config, lib, ... }:

let
  cfg = config.languages.scala;
  java = config.languages.java;
in
{
  options.languages.scala = {
    enable = lib.mkEnableOption "tools for Scala development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.scala_3;
      defaultText = "pkgs.scala_3";
      description = ''
        The Scala package to use.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      (cfg.package.override
        { jre = java.jdk.package; })
      (scala-cli.override
        { jre = java.jdk.package; })
      (sbt.override
        { jre = java.jdk.package; })
      (metals.override
        { jre = java.jdk.package; })
      (coursier.override
        { jre = java.jdk.package; })
      (scalafmt.override
        { jre = java.jdk.package; })
    ];

    languages.java.enable = true;
  };
}
